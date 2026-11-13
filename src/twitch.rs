// Location: neko-overlays-plugin/src/twitch.rs
// File: twitch.rs

//! Talks to Twitch directly: login (device code flow) and live channel data.
//!
//! One background thread logs in and polls Twitch's API (follower count and newest follower,
//! subscriber count, viewers). A second thread (chat.rs) listens to your chat for subscriptions,
//! cheers and watch streaks. Both write into the same snapshot, which the local server (server.rs)
//! hands to the info bar page. OBS's render thread never waits on the network.
//!
//! Login uses Twitch's "device code" flow, which needs no client secret: you register a
//! *public* app at https://dev.twitch.tv/console, build its Client ID into the plugin (or paste
//! it into the source properties), click "Connect to Twitch", and approve the code shown on the bar.

use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::{config_file, log_msg};

/// The public Twitch app everyone logs in through, baked in at build time. build.rs reads it from
/// the NEKO_TWITCH_CLIENT_ID environment variable or from `.env`. A Client ID is not a secret.
const DEFAULT_CLIENT_ID: &str = match option_env!("NEKO_TWITCH_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// chat:read is for the chat connection (subscriptions, cheers, watch streaks).
const SCOPES: &str = "moderator:read:followers channel:read:subscriptions bits:read chat:read";
const TOKEN_FILE: &str = "twitch.txt";
const LATEST_FILE: &str = "latest.json";
const POLL_EVERY: Duration = Duration::from_secs(15);
const RETRY_EVERY: Duration = Duration::from_secs(20);

// --- Public API ------------------------------------------------------------------

/// One "latest ..." entry. `amount` is months (subscriber), bits (cheer) or streams in a row (streaker).
/// `seq` only ever goes up, so the page can tell a new event from an old one.
#[derive(Clone, Default)]
pub struct Latest {
    pub name: String,
    pub amount: Option<u64>,
    pub seq: u64,
}

/// One current subscriber, as listed by Twitch. `tier` is 1, 2 or 3.
#[derive(Clone)]
pub struct SubscriberEntry {
    pub name: String,
    pub tier: u8,
}

/// What the info bar shows. `status` is "Label|Value" while the user has something to do
/// (or something is wrong); empty when everything is fine.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub status: String,
    /// The code the user has to enter on Twitch, while a login is waiting for them.
    pub login: Option<LoginPrompt>,
    pub followers: Option<u64>,
    pub subscribers: Option<u64>,
    /// Everyone subscribed right now (not the broadcaster), for the bar to rotate through.
    pub subscriber_list: Vec<SubscriberEntry>,
    pub viewers: Option<u64>,
    pub latest_follower: Option<Latest>,
    pub latest_subscriber: Option<Latest>,
    pub latest_cheer: Option<Latest>,
    pub latest_streaker: Option<Latest>,
}

/// The code (and the address Twitch gave) shown on the local login page while a login is waiting.
#[derive(Clone)]
pub struct LoginPrompt {
    pub code: String,
    pub uri: String,
}

/// Which "latest" slot an event belongs to.
#[derive(Clone, Copy)]
pub enum Slot {
    Follower,
    Subscriber,
    Cheer,
    Streaker,
}

/// What the chat thread needs to log in to chat.
#[derive(Clone)]
pub struct ChatAuth {
    pub login: String,
    pub token: String,
    /// Changes when somebody logs in or out, so the chat thread knows to reconnect.
    pub gen: u64,
}

pub fn start() {
    load_latest();
    let shared = Arc::clone(shared());
    shared.stop.store(false, Ordering::SeqCst);
    match thread::Builder::new().name("neko-twitch".into()).spawn(move || run(shared)) {
        Ok(handle) => {
            if let Ok(mut worker) = WORKER.lock() {
                *worker = Some(handle);
            }
        }
        Err(_) => log_msg("[neko-overlays-plugin] could not start the Twitch thread"),
    }
    crate::chat::start();
}

/// Stops the workers and waits for them. Must happen before OBS unloads the plugin.
pub fn stop() {
    shared().stop.store(true, Ordering::SeqCst);
    crate::chat::stop();
    let handle = WORKER.lock().ok().and_then(|mut worker| worker.take());
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

pub fn stopping() -> bool {
    shared().stop.load(Ordering::SeqCst)
}

/// Sleeps in short slices so shutdown never waits long.
pub fn pause(total: Duration) {
    nap(shared(), total);
}

pub fn set_client_id(id: &str) {
    if let Ok(mut current) = shared().client_id.lock() {
        if *current != id {
            *current = id.to_string();
        }
    }
}

pub fn request_login() {
    // A click while a login is already waiting must not queue a second one: it would start a new login (and a new
    // code) the moment this one is approved. The login page just shows the code that is already there.
    if shared().login_active.load(Ordering::SeqCst) {
        return;
    }
    shared().login_requested.store(true, Ordering::SeqCst);
}

pub fn request_logout() {
    shared().logout_requested.store(true, Ordering::SeqCst);
}

pub fn snapshot() -> Snapshot {
    shared().snapshot.lock().map(|s| s.clone()).unwrap_or_default()
}

pub fn chat_auth() -> Option<ChatAuth> {
    shared().chat_auth.lock().ok().and_then(|auth| auth.clone())
}

/// The chat thread reports an event here. It becomes the new "latest ..." of its kind.
pub fn record_event(slot: Slot, name: &str, amount: Option<u64>) {
    shared().update(|snap| {
        let target = slot_mut(snap, slot);
        let seq = target.as_ref().map_or(0, |latest| latest.seq) + 1;
        *target = Some(Latest { name: name.to_string(), amount, seq });
    });
    save_latest();
}

/// Everything the info bar page needs besides its settings: the numbers, the latest events, and
/// the message that takes over the bar while the user has something to do.
pub fn state_json() -> Value {
    let snap = snapshot();
    let status = match snap.status.split_once('|') {
        Some((label, value)) => json!({ "label": label, "value": value }),
        None if !snap.status.is_empty() => json!({ "label": "Twitch", "value": snap.status }),
        None => Value::Null,
    };
    let login = match &snap.login {
        Some(prompt) => json!({ "code": prompt.code, "uri": prompt.uri }),
        None => Value::Null,
    };
    json!({
        "status": status,
        "login": login,
        "counts": {
            "followers": snap.followers.unwrap_or(0),
            "subs": snap.subscribers.unwrap_or(0),
            "viewers": snap.viewers.unwrap_or(0),
        },
        "subscribers": snap
            .subscriber_list
            .iter()
            .map(|entry| json!({ "name": entry.name, "tier": entry.tier }))
            .collect::<Vec<Value>>(),
        "latest": {
            "follower": latest_to_json(&snap.latest_follower),
            "subscriber": latest_to_json(&snap.latest_subscriber),
            "cheer": latest_to_json(&snap.latest_cheer),
            "streaker": latest_to_json(&snap.latest_streaker),
        },
    })
}

// --- Latest events: JSON and the file they are kept in ----------------------------------

fn slot_mut(snap: &mut Snapshot, slot: Slot) -> &mut Option<Latest> {
    match slot {
        Slot::Follower => &mut snap.latest_follower,
        Slot::Subscriber => &mut snap.latest_subscriber,
        Slot::Cheer => &mut snap.latest_cheer,
        Slot::Streaker => &mut snap.latest_streaker,
    }
}

fn latest_to_json(latest: &Option<Latest>) -> Value {
    match latest {
        Some(l) => {
            let mut value = json!({ "name": l.name, "seq": l.seq });
            if let Some(amount) = l.amount {
                value["amount"] = json!(amount);
            }
            value
        }
        None => Value::Null,
    }
}

fn latest_from_json(value: &Value) -> Option<Latest> {
    let name = value["name"].as_str()?.to_string();
    if name.is_empty() {
        return None;
    }
    Some(Latest { name, amount: value["amount"].as_u64(), seq: value["seq"].as_u64().unwrap_or(1).max(1) })
}

/// The latest events survive restarting OBS, so "latest subscriber" is still there tomorrow.
fn save_latest() {
    let snap = snapshot();
    let Some(path) = config_file(LATEST_FILE) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let value = json!({
        "follower": latest_to_json(&snap.latest_follower),
        "subscriber": latest_to_json(&snap.latest_subscriber),
        "cheer": latest_to_json(&snap.latest_cheer),
        "streaker": latest_to_json(&snap.latest_streaker),
    });
    let _ = fs::write(path, value.to_string());
}

fn load_latest() {
    let Some(path) = config_file(LATEST_FILE) else { return };
    let Ok(text) = fs::read_to_string(path) else { return };
    let Ok(value) = serde_json::from_str::<Value>(&text) else { return };
    shared().update(|snap| {
        snap.latest_follower = latest_from_json(&value["follower"]);
        snap.latest_subscriber = latest_from_json(&value["subscriber"]);
        snap.latest_cheer = latest_from_json(&value["cheer"]);
        snap.latest_streaker = latest_from_json(&value["streaker"]);
    });
}

// --- Shared state ------------------------------------------------------------------

struct Shared {
    snapshot: Mutex<Snapshot>,
    version: AtomicU64,
    client_id: Mutex<String>,
    login_requested: AtomicBool,
    /// True while a login is waiting for the user to approve it on Twitch.
    login_active: AtomicBool,
    logout_requested: AtomicBool,
    stop: AtomicBool,
    chat_auth: Mutex<Option<ChatAuth>>,
    auth_gen: AtomicU64,
}

impl Shared {
    fn update(&self, change: impl FnOnce(&mut Snapshot)) {
        if let Ok(mut snap) = self.snapshot.lock() {
            change(&mut snap);
        }
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// Only bumps the version when the text actually changes.
    fn set_status(&self, status: &str) {
        let changed = match self.snapshot.lock() {
            Ok(mut snap) if snap.status != status => {
                snap.status = status.to_string();
                true
            }
            _ => false,
        };
        if changed {
            if !status.is_empty() {
                log_msg(&format!("[neko-overlays-plugin] bar status: {status}"));
            }
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Hands the chat thread the current login. `bump` marks a login or logout (it reconnects);
    /// a plain token refresh keeps the same generation, so the chat connection is left alone.
    fn publish_auth(&self, tokens: Option<&Tokens>, bump: bool) {
        if bump {
            self.auth_gen.fetch_add(1, Ordering::SeqCst);
        }
        let value = tokens.filter(|t| !t.login.is_empty()).map(|t| ChatAuth {
            login: t.login.clone(),
            token: t.access.clone(),
            gen: self.auth_gen.load(Ordering::SeqCst),
        });
        if let Ok(mut slot) = self.chat_auth.lock() {
            *slot = value;
        }
    }
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn shared() -> &'static Arc<Shared> {
    SHARED.get_or_init(|| {
        Arc::new(Shared {
            snapshot: Mutex::new(Snapshot::default()),
            version: AtomicU64::new(1),
            client_id: Mutex::new(String::new()),
            login_requested: AtomicBool::new(false),
            login_active: AtomicBool::new(false),
            logout_requested: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            chat_auth: Mutex::new(None),
            auth_gen: AtomicU64::new(1),
        })
    })
}

/// Sleeps in short slices so shutdown never waits long.
fn nap(shared: &Shared, total: Duration) {
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < total && !shared.stop.load(Ordering::SeqCst) {
        thread::sleep(step);
        waited += step;
    }
}

// --- Tokens --------------------------------------------------------------------------

struct Tokens {
    access: String,
    refresh: String,
    user_id: String,
    login: String,
}

fn load_tokens() -> Option<Tokens> {
    let text = fs::read_to_string(config_file(TOKEN_FILE)?).ok()?;
    let mut lines = text.lines();
    let access = lines.next()?.to_string();
    let refresh = lines.next()?.to_string();
    let user_id = lines.next().unwrap_or("").to_string();
    let login = lines.next().unwrap_or("").to_string();
    if access.is_empty() || refresh.is_empty() {
        return None;
    }
    Some(Tokens { access, refresh, user_id, login })
}

fn save_tokens(tokens: &Tokens) {
    let Some(path) = config_file(TOKEN_FILE) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text = format!("{}\n{}\n{}\n{}\n", tokens.access, tokens.refresh, tokens.user_id, tokens.login);
    if fs::write(&path, text).is_err() {
        log_msg("[neko-overlays-plugin] could not save the Twitch login");
        return;
    }
    // The file holds a login token, so keep it private to this user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
}

fn delete_tokens() {
    if let Some(path) = config_file(TOKEN_FILE) {
        let _ = fs::remove_file(path);
    }
}

// --- Worker ----------------------------------------------------------------------------

enum PollError {
    /// The refresh token no longer works; the user has to connect again.
    LoginExpired,
    Other(String),
}

impl PollError {
    fn into_message(self) -> String {
        match self {
            PollError::LoginExpired => String::from("Twitch login expired"),
            PollError::Other(message) => message,
        }
    }
}

fn run(shared: Arc<Shared>) {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("neko-overlays-plugin")
        .build();
    let mut tokens = load_tokens();
    shared.publish_auth(tokens.as_ref(), true);
    let mut next_poll = Instant::now();

    while !shared.stop.load(Ordering::SeqCst) {
        if shared.logout_requested.swap(false, Ordering::SeqCst) {
            tokens = None;
            delete_tokens();
            shared.publish_auth(None, true);
            shared.update(|snap| {
                snap.status = String::new();
                snap.followers = None;
                snap.subscribers = None;
                snap.subscriber_list.clear();
                snap.viewers = None;
            });
        }

        // The source property overrides the built-in Client ID (handy for testing your own app).
        let override_id = shared.client_id.lock().map(|id| id.clone()).unwrap_or_default();
        let client_id = if override_id.is_empty() { DEFAULT_CLIENT_ID.to_string() } else { override_id };
        if client_id.is_empty() {
            shared.set_status("Twitch|No Client ID: paste yours in the properties");
            nap(&shared, Duration::from_millis(500));
            continue;
        }

        if shared.login_requested.swap(false, Ordering::SeqCst) {
            shared.login_active.store(true, Ordering::SeqCst);
            let outcome = device_login(&agent, &client_id, &shared);
            shared.login_active.store(false, Ordering::SeqCst);
            shared.login_requested.store(false, Ordering::SeqCst); // clicks that came in during the login are dropped
            shared.update(|snap| snap.login = None);
            match outcome {
                Ok(new_tokens) => {
                    save_tokens(&new_tokens);
                    log_msg(&format!("[neko-overlays-plugin] connected to Twitch as {}", new_tokens.login));
                    shared.publish_auth(Some(&new_tokens), true);
                    tokens = Some(new_tokens);
                    next_poll = Instant::now();
                    shared.set_status("");
                }
                Err(message) => {
                    log_msg(&format!("[neko-overlays-plugin] {message}"));
                    shared.set_status(&format!("Twitch|{}", truncate(&message, 70)));
                    nap(&shared, Duration::from_secs(3));
                }
            }
            continue;
        }

        let Some(current) = tokens.as_mut() else {
            shared.set_status("Twitch|Not connected: click Connect to Twitch in the properties");
            nap(&shared, Duration::from_millis(500));
            continue;
        };

        if Instant::now() >= next_poll {
            match poll_stats(&agent, &client_id, current, &shared) {
                Ok(()) => {
                    shared.publish_auth(Some(&*current), false); // the token may have been refreshed
                    shared.set_status("");
                    next_poll = Instant::now() + POLL_EVERY;
                }
                Err(PollError::LoginExpired) => {
                    tokens = None;
                    delete_tokens();
                    shared.publish_auth(None, true);
                    shared.set_status("Twitch|Login expired: click Connect to Twitch again");
                }
                Err(PollError::Other(message)) => {
                    log_msg(&format!("[neko-overlays-plugin] Twitch error: {message}"));
                    shared.set_status(&format!("Twitch|{}", truncate(&message, 70)));
                    next_poll = Instant::now() + RETRY_EVERY;
                }
            }
        }
        nap(&shared, Duration::from_millis(250));
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(3)).collect();
    format!("{cut}...")
}

// --- Login (device code flow) ---------------------------------------------------------

fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            format!("HTTP {code}: {body}")
        }
        other => other.to_string(),
    }
}

fn device_login(agent: &ureq::Agent, client_id: &str, shared: &Shared) -> Result<Tokens, String> {
    let start: Value = agent
        .post("https://id.twitch.tv/oauth2/device")
        .send_form(&[("client_id", client_id), ("scopes", SCOPES)])
        .map_err(describe)?
        .into_json()
        .map_err(|e| e.to_string())?;

    let device_code = start["device_code"]
        .as_str()
        .ok_or("Twitch did not return a device code")?
        .to_string();
    let user_code = start["user_code"].as_str().unwrap_or("?").to_string();
    let uri = start["verification_uri"].as_str().unwrap_or("https://www.twitch.tv/activate");
    let interval = start["interval"].as_u64().unwrap_or(5).max(1);
    let expires_in = start["expires_in"].as_u64().unwrap_or(600);

    log_msg(&format!(
        "[neko-overlays-plugin] Connect Twitch: open {uri} and enter code {user_code}  (or open http://127.0.0.1:{}/connect to copy the code)",
        crate::server::port()
    ));
    shared.update(|snap| snap.login = Some(LoginPrompt { code: user_code.clone(), uri: uri.to_string() }));
    shared.set_status(&format!("Connect Twitch|twitch.tv/activate  code {user_code}"));

    let deadline = Instant::now() + Duration::from_secs(expires_in);
    loop {
        nap(shared, Duration::from_secs(interval));
        if shared.stop.load(Ordering::SeqCst) {
            return Err(String::from("stopped"));
        }
        // "Disconnect from Twitch" while a code is waiting: drop it. Then Connect gives a fresh one.
        if shared.logout_requested.load(Ordering::SeqCst) {
            return Err(String::from("Login cancelled"));
        }
        if Instant::now() >= deadline {
            return Err(String::from("Login timed out: click Connect to Twitch again"));
        }

        let result = agent.post("https://id.twitch.tv/oauth2/token").send_form(&[
            ("client_id", client_id),
            ("scopes", SCOPES),
            ("device_code", device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);

        match result {
            Ok(response) => {
                let body: Value = response.into_json().map_err(|e| e.to_string())?;
                return finish_login(agent, client_id, &body);
            }
            Err(ureq::Error::Status(_, response)) => {
                let body: Value = response.into_json().unwrap_or_default();
                match body["message"].as_str() {
                    // Still waiting for the user to approve in the browser.
                    Some("authorization_pending") | Some("slow_down") => continue,
                    Some(other) => return Err(format!("Twitch login failed: {other}")),
                    None => return Err(String::from("Twitch login failed")),
                }
            }
            // A network hiccup while waiting: just try again on the next tick.
            Err(_) => continue,
        }
    }
}

fn finish_login(agent: &ureq::Agent, client_id: &str, body: &Value) -> Result<Tokens, String> {
    let access = body["access_token"]
        .as_str()
        .ok_or("Twitch did not return an access token")?
        .to_string();
    let refresh = body["refresh_token"].as_str().unwrap_or("").to_string();
    let mut tokens = Tokens { access, refresh, user_id: String::new(), login: String::new() };
    ensure_user(agent, client_id, &mut tokens).map_err(PollError::into_message)?;
    Ok(tokens)
}

fn refresh(agent: &ureq::Agent, client_id: &str, tokens: &mut Tokens) -> Result<(), PollError> {
    if tokens.refresh.is_empty() {
        return Err(PollError::LoginExpired);
    }
    let result = agent.post("https://id.twitch.tv/oauth2/token").send_form(&[
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", tokens.refresh.as_str()),
    ]);
    match result {
        Ok(response) => {
            let body: Value = response.into_json().map_err(|e| PollError::Other(e.to_string()))?;
            tokens.access = body["access_token"]
                .as_str()
                .ok_or(PollError::LoginExpired)?
                .to_string();
            if let Some(new_refresh) = body["refresh_token"].as_str() {
                tokens.refresh = new_refresh.to_string();
            }
            save_tokens(tokens);
            Ok(())
        }
        Err(ureq::Error::Status(400, _)) | Err(ureq::Error::Status(401, _)) => Err(PollError::LoginExpired),
        Err(other) => Err(PollError::Other(describe(other))),
    }
}

// --- Helix (Twitch API) ----------------------------------------------------------------

/// GET https://api.twitch.tv/helix/<path>. Refreshes the token once on a 401.
fn helix_get(
    agent: &ureq::Agent,
    client_id: &str,
    tokens: &mut Tokens,
    path: &str,
) -> Result<Value, PollError> {
    for attempt in 0..2 {
        let result = agent
            .get(&format!("https://api.twitch.tv/helix/{path}"))
            .set("Client-Id", client_id)
            .set("Authorization", &format!("Bearer {}", tokens.access))
            .call();
        match result {
            Ok(response) => {
                return response.into_json::<Value>().map_err(|e| PollError::Other(e.to_string()));
            }
            Err(ureq::Error::Status(401, _)) if attempt == 0 => refresh(agent, client_id, tokens)?,
            Err(other) => return Err(PollError::Other(describe(other))),
        }
    }
    Err(PollError::LoginExpired)
}

/// Looks up which account the token belongs to (needed as the broadcaster id).
fn ensure_user(agent: &ureq::Agent, client_id: &str, tokens: &mut Tokens) -> Result<(), PollError> {
    if !tokens.user_id.is_empty() && !tokens.login.is_empty() {
        return Ok(());
    }
    let body = helix_get(agent, client_id, tokens, "users")?;
    let user = &body["data"][0];
    tokens.user_id = user["id"]
        .as_str()
        .ok_or_else(|| PollError::Other(String::from("Twitch returned no user")))?
        .to_string();
    tokens.login = user["login"].as_str().unwrap_or("").to_string();
    save_tokens(tokens);
    Ok(())
}

/// The subscriber total and everyone who is subscribed right now, without the broadcaster (Twitch lists the
/// channel owner too, but leaves them out of `total`). The list is sorted by tier (highest first), then by name,
/// so the order never depends on what Twitch happens to return.
fn fetch_subscribers(
    agent: &ureq::Agent,
    client_id: &str,
    tokens: &mut Tokens,
    broadcaster_id: &str,
) -> Result<(Option<u64>, Vec<SubscriberEntry>), PollError> {
    const PAGE: usize = 100;
    let mut total = None;
    let mut list: Vec<SubscriberEntry> = Vec::new();
    let mut after = String::new();

    for page in 0..20 {
        let mut path = format!("subscriptions?broadcaster_id={broadcaster_id}&first={PAGE}");
        if !after.is_empty() {
            path.push_str(&format!("&after={after}"));
        }
        let body = helix_get(agent, client_id, tokens, &path)?;
        if page == 0 {
            total = body["total"].as_u64();
        }
        let entries = body["data"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for entry in entries {
            if entry["user_id"].as_str() == Some(broadcaster_id) {
                continue;
            }
            let name = entry["user_name"].as_str().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let tier = match entry["tier"].as_str() {
                Some("3000") => 3,
                Some("2000") => 2,
                _ => 1,
            };
            list.push(SubscriberEntry { name: name.to_string(), tier });
        }
        // Twitch also sends a cursor on the last page, so a short page is what ends the loop.
        match body["pagination"]["cursor"].as_str() {
            Some(cursor) if entries.len() >= PAGE && !cursor.is_empty() => after = cursor.to_string(),
            _ => break,
        }
    }

    list.sort_by(|a, b| b.tier.cmp(&a.tier).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok((total, list))
}

/// The last reason the subscriber count could not be read, so the log gets one line per problem, not one per poll.
static SUBSCRIBER_PROBLEM: Mutex<String> = Mutex::new(String::new());

/// `Some(reason)` when the subscriber request failed, `None` when it worked.
fn note_subscriber_problem(problem: Option<&str>) {
    let Ok(mut last) = SUBSCRIBER_PROBLEM.lock() else { return };
    match problem {
        Some(reason) if *last != reason => {
            let hint = if reason.to_lowercase().contains("scope") {
                " (the login lacks the subscriber permission: click Disconnect, then Connect to Twitch)"
            } else {
                ""
            };
            log_msg(&format!("[neko-overlays-plugin] subscriber count unavailable: {reason}{hint}"));
            *last = reason.to_string();
        }
        Some(_) => {}
        None => {
            if !last.is_empty() {
                log_msg("[neko-overlays-plugin] subscriber count is working again");
                last.clear();
            }
        }
    }
}

fn poll_stats(
    agent: &ureq::Agent,
    client_id: &str,
    tokens: &mut Tokens,
    shared: &Shared,
) -> Result<(), PollError> {
    ensure_user(agent, client_id, tokens)?;
    let id = tokens.user_id.clone();

    // Newest follower comes first, and `total` is the follower count.
    let followers = helix_get(agent, client_id, tokens, &format!("channels/followers?broadcaster_id={id}&first=1"))?;
    let follower_total = followers["total"].as_u64();
    let newest_follower = followers["data"][0]["user_name"].as_str().map(str::to_string);

    // Subscriber stats need their own permission and only exist for affiliates/partners, so a failure here isn't
    // fatal for the other numbers. The last known count is kept, and the reason is written to the OBS log.
    let subscribers = match fetch_subscribers(agent, client_id, tokens, &id) {
        Ok(found) => {
            note_subscriber_problem(if found.0.is_none() { Some("Twitch sent no subscriber total") } else { None });
            Some(found)
        }
        Err(PollError::LoginExpired) => return Err(PollError::LoginExpired),
        Err(PollError::Other(message)) => {
            note_subscriber_problem(Some(&message));
            None
        }
    };

    // Viewers: the stream's viewer count, or 0 while offline. A failure keeps the last number.
    let stream = helix_get(agent, client_id, tokens, &format!("streams?user_id={id}")).ok();
    let viewers = stream.as_ref().map(|body| body["data"][0]["viewer_count"].as_u64().unwrap_or(0));

    let mut follower_changed = false;
    shared.update(|snap| {
        snap.followers = follower_total;
        // A failed call keeps the last number and names instead of showing 0.
        if let Some((total, list)) = subscribers {
            if total.is_some() {
                snap.subscribers = total;
            }
            snap.subscriber_list = list;
        }
        if viewers.is_some() {
            snap.viewers = viewers;
        }
        // A different newest follower than the one we know is a new follower. (Followers are not in chat,
        // so this is found by polling, at most POLL_EVERY late.)
        if let Some(name) = newest_follower {
            let known = snap.latest_follower.as_ref().map(|l| l.name.as_str());
            if known != Some(name.as_str()) {
                let seq = snap.latest_follower.as_ref().map_or(0, |l| l.seq) + 1;
                snap.latest_follower = Some(Latest { name, amount: None, seq });
                follower_changed = true;
            }
        }
    });
    if follower_changed {
        save_latest();
    }
    Ok(())
}