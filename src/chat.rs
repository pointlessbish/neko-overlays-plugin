// Location: neko-overlays-plugin/src/chat.rs
// File: chat.rs

//! Reads your own Twitch chat (IRC over TLS) to find subscriptions, cheers and watch streaks, which
//! feed the "latest subscriber", "latest cheer" and "latest watch streak" slots of the info bar.
//! It only listens: it never sends a chat message.
//!
//! It uses the login from twitch.rs (the `chat:read` permission). If you connected before that
//! permission was added, click Connect to Twitch once more.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

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
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
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
            Ok(()) => backoff = 5, // asked to reconnect, or the login changed
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

/// One connection to chat. Returns Ok when it should simply be opened again.
fn session(config: &Arc<ClientConfig>, auth: &ChatAuth) -> Result<(), String> {
    let address = (HOST, PORT)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| String::from("no address for the chat server"))?;
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    // A short read timeout lets the loop below notice shutdown and a changed login.
    tcp.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(10))).map_err(|e| e.to_string())?;

    let name = ServerName::try_from(HOST).map_err(|e| e.to_string())?;
    let connection = ClientConnection::new(Arc::clone(config), name).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);

    let login = auth.login.to_lowercase();
    send(&mut stream, "CAP REQ :twitch.tv/tags twitch.tv/commands")?;
    send(&mut stream, &format!("PASS oauth:{}", auth.token))?; // never logged
    send(&mut stream, &format!("NICK {login}"))?;
    send(&mut stream, &format!("JOIN #{login}"))?;

    let mut buffer = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    let mut last_data = Instant::now();

    loop {
        if twitch::stopping() {
            return Ok(());
        }
        // Logged out, or logged in as somebody else: leave, the caller connects again with the new login.
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
                // Twitch pings about every 5 minutes, so 6 minutes of silence means the line is dead.
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
    let _prefix = parts.next().unwrap_or("");
    let command = parts.next().unwrap_or("");
    let params = parts.next().unwrap_or("");

    match command {
        "001" => log_msg("[neko-overlays-plugin] chat connected"),
        "RECONNECT" => return Ok(Flow::Reconnect),
        "NOTICE" => {
            if params.contains("authentication failed") || params.contains("Improperly formatted auth") {
                return Err(String::from(
                    "Twitch refused the chat login. Click Connect to Twitch again so the chat permission is granted.",
                ));
            }
        }
        "USERNOTICE" => user_notice(&tags),
        "PRIVMSG" => cheer(&tags),
        _ => {}
    }
    Ok(Flow::Continue)
}

/// "@a=1;b=2 :rest of the line" -> ({a: 1, b: 2}, ":rest of the line")
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

/// IRC tag values escape a few characters.
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

/// Subscriptions, gifted subscriptions and watch streaks.
fn user_notice(tags: &HashMap<&str, String>) {
    let name = display_name(tags);
    match tags.get("msg-id").map(String::as_str) {
        Some("sub") | Some("resub") => {
            let months = number(tags, "msg-param-cumulative-months").unwrap_or(1).max(1);
            twitch::record_event(Slot::Subscriber, &name, Some(months));
        }
        Some("subgift") => {
            let receiver = tags
                .get("msg-param-recipient-display-name")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or(name);
            let months = number(tags, "msg-param-months").unwrap_or(1).max(1);
            twitch::record_event(Slot::Subscriber, &receiver, Some(months));
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

/// A chat message with bits is a cheer.
fn cheer(tags: &HashMap<&str, String>) {
    if let Some(bits) = number(tags, "bits") {
        if bits > 0 {
            twitch::record_event(Slot::Cheer, &display_name(tags), Some(bits));
        }
    }
}
