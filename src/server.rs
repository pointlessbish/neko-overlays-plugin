// Location: neko-overlays-plugin/src/server.rs
// File: server.rs

//! The local web server. It serves the info bar page to OBS's browser source, and nothing else.
//!
//! - It listens on 127.0.0.1 only, so nothing outside this computer can reach it.
//! - Everything the page needs is inside the plugin (HTML, JS, the compiled styles, the Stargate artwork).
//!   The only files read from disk are the fonts in the plugin's config folder. Google fonts are
//!   downloaded into that folder the first time a style asks for one (fonts.rs); the page itself never
//!   contacts Google.
//! - Nothing here leaves the computer, and nothing secret is served: the page gets the numbers and
//!   names shown on the bar and the bar's settings, never a Twitch token.
//!
//! Routes: /bar/<id> (the bar), /api/bar/<id>/state, /bar.css, /bar.js, /fonts.css, /fonts/<file>,
//! /api/font?family=<name>, /api/themes and / (a gallery of every style with made-up numbers),
//! plus /connect and /api/login (the page where the Twitch login code can be copied).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};
use tiny_http::{Header, Request, Response, Server, StatusCode};

use crate::{config_file, fonts, log_msg, twitch};

/// The first port tried. If it is busy the next ones are tried, and the bar is pointed at whichever worked.
const FIRST_PORT: u16 = 47831;
const PORT_TRIES: u16 = 20;

/// (id, label). The ids are the data-theme names in the stylesheets. The default comes first.
pub const THEMES: [(&str, &str); 12] = [
    ("classic", "Classic"),
    ("glass", "Glass"),
    ("card", "Card"),
    ("minimal", "Minimal"),
    ("neon", "Neon sign"),
    ("terminal", "Terminal"),
    ("win98", "Windows 98"),
    ("pirate", "Pirate"),
    ("phasmo", "Evidence board"),
    ("pill", "Midnight"),
    ("ocean", "Ocean"),
    ("finals", "THE FINALS"),
];

const BAR_HTML: &str = include_str!("../assets/bar.html");
const GALLERY_HTML: &str = include_str!("../assets/gallery.html");
const CONNECT_HTML: &str = include_str!("../assets/connect.html");
const BAR_JS: &str = include_str!("../assets/bar.js");
const BAR_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/infobar.css"));
const EXTRA_CSS: &str = include_str!("../assets/bar-extra.css");
const STARGATE_LINES: &str = include_str!("../assets/stargate-lines.svg");
const STARGATE_STARS: &str = include_str!("../assets/stargate-stars.svg");

static PORT: AtomicU16 = AtomicU16::new(0);
static STOP: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

// --- The bars (one per info bar source in OBS) ---------------------------------------------

struct Bar {
    settings: Value,
    /// Changes whenever the settings change, so the page knows to rebuild itself.
    version: u64,
}

static BARS: OnceLock<Mutex<HashMap<u32, Bar>>> = OnceLock::new();

fn bars() -> &'static Mutex<HashMap<u32, Bar>> {
    BARS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The settings a bar has before anyone changed anything. Same shape as the ones info_bar.rs builds.
pub fn default_settings() -> Value {
    json!({
        "theme": "classic",
        "colors": {},
        "font": null,
        "fontSize": null,
        "googleFonts": true,
        "rotateSeconds": 10,
        "train": { "enabled": true, "type": "subscriber", "seconds": 30 },
        "stats": { "followers": true, "subs": true, "viewers": false },
        "latest": { "follower": true, "subscriber": true, "cheer": true, "streaker": true }
    })
}

/// Called when an info bar source is created. Returns the id its page is served under.
pub fn register_bar() -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut all) = bars().lock() {
        all.insert(id, Bar { settings: default_settings(), version: 1 });
    }
    id
}

pub fn update_bar(id: u32, settings: Value) {
    if let Ok(mut all) = bars().lock() {
        if let Some(bar) = all.get_mut(&id) {
            bar.settings = settings;
            bar.version += 1;
        }
    }
}

pub fn remove_bar(id: u32) {
    if let Ok(mut all) = bars().lock() {
        all.remove(&id);
    }
}

/// 0 when the server could not start.
pub fn port() -> u16 {
    PORT.load(Ordering::SeqCst)
}

/// The address OBS's browser source loads for a bar.
pub fn bar_url(id: u32) -> String {
    format!("http://127.0.0.1:{}/bar/{id}", port())
}

// --- Lifecycle -------------------------------------------------------------------------------

pub fn start() {
    STOP.store(false, Ordering::SeqCst);

    let mut server = None;
    for offset in 0..PORT_TRIES {
        let port = FIRST_PORT + offset;
        if let Ok(found) = Server::http(("127.0.0.1", port)) {
            PORT.store(port, Ordering::SeqCst);
            server = Some(found);
            break;
        }
    }
    let Some(server) = server else {
        log_msg("[neko-overlays-plugin] could not start the local server: ports 47831-47850 are all busy");
        return;
    };
    log_msg(&format!(
        "[neko-overlays-plugin] local server on http://127.0.0.1:{0}  (style gallery: http://127.0.0.1:{0}/)",
        port()
    ));

    match thread::Builder::new().name("neko-server".into()).spawn(move || serve(server)) {
        Ok(handle) => {
            if let Ok(mut worker) = WORKER.lock() {
                *worker = Some(handle);
            }
        }
        Err(_) => log_msg("[neko-overlays-plugin] could not start the server thread"),
    }
}

/// Stops the server and waits for it. Must happen before OBS unloads the plugin.
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
    let handle = WORKER.lock().ok().and_then(|mut worker| worker.take());
    if let Some(handle) = handle {
        let _ = handle.join();
    }
    PORT.store(0, Ordering::SeqCst);
}

fn serve(server: Server) {
    while !STOP.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(250)) {
            Ok(Some(request)) => handle(request),
            Ok(None) => {}
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    }
}

// --- Requests ---------------------------------------------------------------------------------

fn reply(request: Request, status: u16, content_type: &str, body: Vec<u8>) {
    let mut response = Response::from_data(body).with_status_code(StatusCode(status));
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
        response.add_header(header);
    }
    if let Ok(header) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
        response.add_header(header);
    }
    let _ = request.respond(response);
}

fn reply_text(request: Request, content_type: &str, body: &str) {
    reply(request, 200, content_type, body.as_bytes().to_vec());
}

fn not_found(request: Request) {
    reply(request, 404, "text/plain; charset=utf-8", b"Not found".to_vec());
}

/// Only requests addressed to this computer are answered. A web page cannot make a browser send
/// anything else to 127.0.0.1, unless it tricks it with a DNS name that points here (DNS rebinding).
fn host_is_local(request: &Request) -> bool {
    request.headers().iter().any(|header| {
        if !header.field.equiv("Host") {
            return false;
        }
        let host = header.value.as_str();
        let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        name == "127.0.0.1" || name == "localhost"
    })
}

fn handle(request: Request) {
    if !host_is_local(&request) {
        reply(request, 403, "text/plain; charset=utf-8", b"Forbidden".to_vec());
        return;
    }

    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    match path.as_str() {
        "/" | "/styles" => reply_text(request, "text/html; charset=utf-8", GALLERY_HTML),
        "/bar.js" => reply_text(request, "text/javascript; charset=utf-8", BAR_JS),
        "/bar.css" => {
            let css = format!("{BAR_CSS}\n{EXTRA_CSS}");
            reply_text(request, "text/css; charset=utf-8", &css);
        }
        "/stargate-lines.svg" => reply_text(request, "image/svg+xml", STARGATE_LINES),
        "/stargate-stars.svg" => reply_text(request, "image/svg+xml", STARGATE_STARS),
        "/fonts.css" => {
            // Every font downloaded so far (fonts.rs). Until a font is there the style uses a fallback.
            reply(request, 200, "text/css; charset=utf-8", fonts::combined_css());
        }
        "/api/font" => {
            // The page asks for a Google font by name; the download runs in the background and the page asks again.
            let family = query_value(&url, "family").unwrap_or_default();
            let status = fonts::request(&family);
            let body = json!({ "status": status.as_str(), "generation": fonts::generation() });
            reply_text(request, "application/json", &body.to_string());
        }
        "/connect" => reply_text(request, "text/html; charset=utf-8", CONNECT_HTML),
        "/api/login" => reply_text(request, "application/json", &twitch::state_json().to_string()),
        "/api/themes" => {
            let list: Vec<Value> = THEMES.iter().map(|(id, label)| json!({ "id": id, "label": label })).collect();
            reply_text(request, "application/json", &Value::Array(list).to_string());
        }
        _ => {
            if let Some(name) = path.strip_prefix("/fonts/") {
                match read_font_file(name) {
                    Some(bytes) => reply(request, 200, font_type(name), bytes),
                    None => not_found(request),
                }
            } else if path.starts_with("/bar/") {
                reply_text(request, "text/html; charset=utf-8", BAR_HTML);
            } else if let Some(id) = path.strip_prefix("/api/bar/").and_then(|rest| rest.strip_suffix("/state")) {
                match bar_state(id) {
                    Some(state) => reply_text(request, "application/json", &state.to_string()),
                    None => not_found(request),
                }
            } else {
                not_found(request);
            }
        }
    }
}

/// What the page polls: the bar's settings and version, plus the numbers and latest events.
fn bar_state(id: &str) -> Option<Value> {
    let id: u32 = id.parse().ok()?;
    let (settings, version) = if id == 0 {
        (default_settings(), 1) // the gallery's bar: it is not tied to a source
    } else {
        let all = bars().lock().ok()?;
        let bar = all.get(&id)?;
        (bar.settings.clone(), bar.version)
    };
    let mut state = twitch::state_json();
    state["v"] = json!(version);
    state["settings"] = settings;
    Some(state)
}

// --- Fonts ----------------------------------------------------------------------------------------

/// The value of one query parameter, with %XX and + decoded.
fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let raw = query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })?;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn font_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

/// A file from the plugin's `fonts` config folder. Only plain file names are accepted.
fn read_font_file(name: &str) -> Option<Vec<u8>> {
    let plain = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if !plain {
        return None;
    }
    std::fs::read(config_file(&format!("fonts/{name}"))?).ok()
}

/// Opens the login page (/connect) in the default browser, so the Twitch code can be copied instead of typed.
/// If it cannot be opened, the address is in the OBS log and the user can open it themselves.
pub fn open_connect_page() {
    let port = port();
    if port == 0 {
        return;
    }
    let url = format!("http://127.0.0.1:{port}/connect");

    #[cfg(windows)]
    let started = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url.as_str()]).spawn();
    #[cfg(not(windows))]
    let started = std::process::Command::new("xdg-open").arg(url.as_str()).spawn();

    match started {
        Ok(mut child) => {
            // Collect the finished helper process, so it does not linger.
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(_) => log_msg(&format!("[neko-overlays-plugin] could not open a browser: open {url} yourself")),
    }
}
