use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{json, Value};

use crate::alerts;
use crate::log_msg;
use crate::twitch::{self, ChatAuth, Slot};

const HOST: &str = "irc.chat.twitch.tv";
const PORT: u16 = 6697;

static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub fn start() {
    match thread::Builder::new().name("neko-chat".into()).spawn(run) {
        Ok(handle) => {
            if let Ok(mut worker) = WORKER.lock() {
                *worker = Some(handle);
            }
        }
        Err(_) => log_msg("[neko-overlays-plugin] could not start the chat thread"),
    }
}

pub fn stop() {
    let handle = WORKER.lock().ok().and_then(|mut worker| worker.take());
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

fn tls_config() -> Option<Arc<ClientConfig>> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Some(Arc::new(config))
}

fn run() {
    let Some(config) = tls_config() else {
        log_msg("[neko-overlays-plugin] chat: could not set up TLS, subscribers, cheers and streaks will not update");
        return;
    };
    let mut backoff = 5u64;
    while !twitch::stopping() {
        let Some(auth) = twitch::chat_auth() else {
            twitch::pause(Duration::from_secs(1));
            continue;
        };
        match session(&config, &auth) {
            Ok(()) => backoff = 5,
            Err(message) => {
                log_msg(&format!("[neko-overlays-plugin] chat: {message}"));
                twitch::pause(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(120);
            }
        }
    }
}

enum Flow {
    Continue,
    Reconnect,
}

fn session(config: &Arc<ClientConfig>, auth: &ChatAuth) -> Result<(), String> {
    let address = (HOST, PORT)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| String::from("no address for the chat server"))?;
    let tcp =
        TcpStream::connect_timeout(&address, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let name = ServerName::try_from(HOST).map_err(|e| e.to_string())?;
    let connection = ClientConnection::new(Arc::clone(config), name).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);

    let login = auth.login.to_lowercase();
    use_channel(&login);
    send(&mut stream, "CAP REQ :twitch.tv/tags twitch.tv/commands")?;
    send(&mut stream, &format!("PASS oauth:{}", auth.token))?;
    send(&mut stream, &format!("NICK {login}"))?;
    send(&mut stream, &format!("JOIN #{login}"))?;

    let mut buffer = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    let mut last_data = Instant::now();

    loop {
        if twitch::stopping() {
            return Ok(());
        }
        if twitch::chat_auth().map_or(true, |current| current.gen != auth.gen) {
            return Ok(());
        }

        match stream.read(&mut buffer) {
            Ok(0) => return Err(String::from("the connection was closed")),
            Ok(count) => {
                last_data = Instant::now();
                pending.extend_from_slice(&buffer[..count]);
                while let Some(end) = pending.windows(2).position(|pair| pair == b"\r\n") {
                    let raw: Vec<u8> = pending.drain(..end + 2).collect();
                    let line = String::from_utf8_lossy(&raw[..raw.len() - 2]).into_owned();
                    if let Flow::Reconnect = handle_line(&mut stream, &line)? {
                        return Ok(());
                    }
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if last_data.elapsed() > Duration::from_secs(360) {
                    return Err(String::from("no data from chat for 6 minutes"));
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn send(stream: &mut impl Write, line: &str) -> Result<(), String> {
    stream
        .write_all(format!("{line}\r\n").as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|e| e.to_string())
}

fn handle_line(stream: &mut impl Write, line: &str) -> Result<Flow, String> {
    if let Some(rest) = line.strip_prefix("PING") {
        send(stream, &format!("PONG{rest}"))?;
        return Ok(Flow::Continue);
    }

    let (tags, rest) = split_tags(line);
    let mut parts = rest.splitn(3, ' ');
    let prefix = parts.next().unwrap_or("");
    let command = parts.next().unwrap_or("");
    let params = parts.next().unwrap_or("");

    match command {
        "001" => log_msg("[neko-overlays-plugin] chat connected"),
        "RECONNECT" => return Ok(Flow::Reconnect),
        "NOTICE" => {
            if params.contains("authentication failed")
                || params.contains("Improperly formatted auth")
            {
                return Err(String::from(
                    "Twitch refused the chat login. Click Connect to Twitch again so the chat permission is granted.",
                ));
            }
        }
        "USERNOTICE" => user_notice(&tags, params),
        "PRIVMSG" => {
            cheer(&tags, params);
            chat_message(&tags, prefix, params);
        }
        "CLEARMSG" => {
            if let Some(id) = tags.get("target-msg-id") {
                remove_message(id);
            }
        }
        "CLEARCHAT" => match tags.get("target-user-id") {
            Some(user_id) if !user_id.is_empty() => remove_user(user_id),
            _ => clear_log(),
        },
        "ROOMSTATE" => {
            if let Some(id) = tags.get("room-id") {
                set_room(id);
            }
        }
        _ => {}
    }
    Ok(Flow::Continue)
}

fn split_tags(line: &str) -> (HashMap<&str, String>, &str) {
    let Some(stripped) = line.strip_prefix('@') else {
        return (HashMap::new(), line);
    };
    let (raw, rest) = stripped.split_once(' ').unwrap_or((stripped, ""));
    let tags = raw
        .split(';')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key, unescape(value)))
        })
        .collect();
    (tags, rest)
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn number(tags: &HashMap<&str, String>, key: &str) -> Option<u64> {
    tags.get(key)?.parse().ok()
}

fn display_name(tags: &HashMap<&str, String>) -> String {
    ["display-name", "login"]
        .iter()
        .filter_map(|key| tags.get(*key))
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| String::from("someone"))
}

// The viewer's own words come after " :" ("#channel :message"). Most notices have none.
fn trailing_text(params: &str) -> &str {
    params
        .split_once(" :")
        .map(|(_, text)| text.trim())
        .unwrap_or("")
}

// The sender's chat colour ("#rrggbb"), empty when they never picked one.
fn user_color(tags: &HashMap<&str, String>) -> String {
    tags.get("color").cloned().unwrap_or_default()
}

fn user_notice(tags: &HashMap<&str, String>, params: &str) {
    let name = display_name(tags);
    let plan = tags
        .get("msg-param-sub-plan")
        .map(String::as_str)
        .unwrap_or("");
    match tags.get("msg-id").map(String::as_str) {
        Some(id @ ("sub" | "resub")) => {
            let months = number(tags, "msg-param-cumulative-months")
                .unwrap_or(1)
                .max(1);
            twitch::record_event(Slot::Subscriber, &name, Some(months));
            let kind = if id == "sub" {
                alerts::SUB
            } else {
                alerts::RESUB
            };
            alerts::push(
                kind,
                &name,
                Some(months),
                plan,
                trailing_text(params),
                "",
                &user_color(tags),
            );
        }
        Some("subgift") => {
            let receiver = tags
                .get("msg-param-recipient-display-name")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| name.clone());
            let months = number(tags, "msg-param-months").unwrap_or(1).max(1);
            twitch::record_event(Slot::Subscriber, &receiver, Some(months));
            // Part of a mass gift: the "submysterygift" notice already made one alert for all of them.
            let in_mass_gift = tags
                .get("msg-param-community-gift-id")
                .map_or(false, |id| !id.is_empty());
            if !in_mass_gift {
                alerts::push(
                    alerts::GIFT,
                    &name,
                    Some(1),
                    plan,
                    "",
                    &receiver,
                    &user_color(tags),
                );
            }
        }
        Some("submysterygift") => {
            let count = number(tags, "msg-param-mass-gift-count")
                .unwrap_or(1)
                .max(1);
            alerts::push(
                alerts::MYSTERY_GIFT,
                &name,
                Some(count),
                plan,
                "",
                "",
                &user_color(tags),
            );
        }
        Some("raid") => {
            let raider = tags
                .get("msg-param-displayName")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or(name);
            let viewers = number(tags, "msg-param-viewerCount").unwrap_or(0);
            alerts::push(
                alerts::RAID,
                &raider,
                Some(viewers),
                "",
                "",
                "",
                &user_color(tags),
            );
        }
        Some("viewermilestone") => {
            if tags.get("msg-param-category").map(String::as_str) == Some("watch-streak") {
                if let Some(streak) = number(tags, "msg-param-value") {
                    twitch::record_event(Slot::Streaker, &name, Some(streak));
                }
            }
        }
        _ => {}
    }
}

fn cheer(tags: &HashMap<&str, String>, params: &str) {
    if let Some(bits) = number(tags, "bits") {
        if bits > 0 {
            let name = display_name(tags);
            twitch::record_event(Slot::Cheer, &name, Some(bits));
            alerts::push(
                alerts::CHEER,
                &name,
                Some(bits),
                "",
                trailing_text(params),
                "",
                &user_color(tags),
            );
        }
    }
}

const LOG_LIMIT: usize = 100;

struct ChatLine {
    seq: u64,
    time_ms: u64,
    id: String,
    user_id: String,
    login: String,
    name: String,
    color: String,
    text: String,
    action: bool,
    emotes: Vec<(String, u64, u64)>,
    badges: Vec<(String, String)>,
}

static LOG: Mutex<VecDeque<ChatLine>> = Mutex::new(VecDeque::new());
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);
static CHANNEL: Mutex<String> = Mutex::new(String::new());
static ROOM: Mutex<String> = Mutex::new(String::new());

fn use_channel(login: &str) {
    if let Ok(mut current) = CHANNEL.lock() {
        if *current != login {
            *current = login.to_string();
            clear_log();
            if let Ok(mut room) = ROOM.lock() {
                room.clear();
            }
        }
    }
}

fn set_room(id: &str) {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    if let Ok(mut room) = ROOM.lock() {
        if *room != id {
            *room = id.to_string();
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn clear_log() {
    if let Ok(mut log) = LOG.lock() {
        log.clear();
    }
}

fn remove_message(id: &str) {
    if let Ok(mut log) = LOG.lock() {
        log.retain(|line| line.id != id);
    }
}

fn remove_user(user_id: &str) {
    if let Ok(mut log) = LOG.lock() {
        log.retain(|line| line.user_id != user_id);
    }
}

fn parse_emotes(raw: &str) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    for group in raw.split('/') {
        let Some((id, ranges)) = group.split_once(':') else {
            continue;
        };
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        for range in ranges.split(',') {
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            if let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) {
                out.push((id.to_string(), start, end));
            }
        }
    }
    out.sort_by_key(|entry| entry.1);
    out
}

fn parse_badges(raw: &str) -> Vec<(String, String)> {
    let plain = |text: &str| {
        !text.is_empty()
            && text.len() <= 40
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    raw.split(',')
        .filter_map(|pair| {
            let (set, version) = pair.split_once('/')?;
            if plain(set) && plain(version) {
                Some((set.to_string(), version.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn chat_message(tags: &HashMap<&str, String>, prefix: &str, params: &str) {
    let Some((_, text)) = params.split_once(" :") else {
        return;
    };
    // "/me waves" arrives as \x01ACTION waves\x01; emote positions count from after "ACTION ".
    let (text, action) = match text.strip_prefix("\u{1}ACTION ") {
        Some(inner) => (inner.trim_end_matches('\u{1}'), true),
        None => (text, false),
    };
    let login = prefix
        .trim_start_matches(':')
        .split('!')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if login.is_empty() || text.is_empty() {
        return;
    }
    if let Some(room) = tags.get("room-id") {
        set_room(room);
    }

    let color = tags
        .get("color")
        .filter(|value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .cloned()
        .unwrap_or_default();
    let line = ChatLine {
        seq: NEXT_SEQ.fetch_add(1, Ordering::SeqCst),
        time_ms: now_ms(),
        id: tags.get("id").cloned().unwrap_or_default(),
        user_id: tags.get("user-id").cloned().unwrap_or_default(),
        name: display_name_or(tags, &login),
        login,
        color,
        text: text.to_string(),
        action,
        emotes: parse_emotes(tags.get("emotes").map(String::as_str).unwrap_or("")),
        badges: parse_badges(tags.get("badges").map(String::as_str).unwrap_or("")),
    };

    if let Ok(mut log) = LOG.lock() {
        log.push_back(line);
        while log.len() > LOG_LIMIT {
            log.pop_front();
        }
    }
}

fn display_name_or(tags: &HashMap<&str, String>, login: &str) -> String {
    tags.get("display-name")
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| login.to_string())
}

fn line_json(line: &ChatLine) -> Value {
    json!({
        "seq": line.seq,
        "ts": line.time_ms,
        "id": line.id,
        "login": line.login,
        "name": line.name,
        "color": line.color,
        "text": line.text,
        "action": line.action,
        "emotes": line.emotes.iter().map(|(id, start, end)| json!({ "id": id, "start": start, "end": end })).collect::<Vec<Value>>(),
        "badges": line.badges.iter().map(|(set, version)| json!({ "set": set, "v": version })).collect::<Vec<Value>>(),
    })
}

pub fn state_json() -> Value {
    let room = ROOM.lock().map(|room| room.clone()).unwrap_or_default();
    let messages: Vec<Value> = LOG
        .lock()
        .map(|log| log.iter().map(line_json).collect::<Vec<Value>>())
        .unwrap_or_default();
    json!({ "room": room, "messages": messages })
}
