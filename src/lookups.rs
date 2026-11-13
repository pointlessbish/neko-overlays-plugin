use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::{log_msg, twitch};

const TIMEOUT: Duration = Duration::from_secs(4);
const RETRY_AFTER: Duration = Duration::from_secs(60);
const USERS_PER_CALL: usize = 100;
const MAX_LOGINS_PER_REQUEST: usize = 40;
const MAX_CACHED_AVATARS: usize = 3000;

static BADGES: Mutex<Option<HashMap<String, Vec<(String, String)>>>> = Mutex::new(None);
static BADGES_FAILED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static AVATARS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static AVATARS_FAILED: Mutex<Option<Instant>> = Mutex::new(None);

enum Failure {
    NoLogin,
    Other(String),
}

fn helix(path: &str) -> Result<Value, Failure> {
    let auth = twitch::chat_auth().ok_or(Failure::NoLogin)?;
    let client_id = twitch::current_client_id();
    if client_id.is_empty() {
        return Err(Failure::NoLogin);
    }
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    agent
        .get(&format!("https://api.twitch.tv/helix/{path}"))
        .set("Client-Id", &client_id)
        .set("Authorization", &format!("Bearer {}", auth.token))
        .call()
        .map_err(|e| Failure::Other(e.to_string()))?
        .into_json::<Value>()
        .map_err(|e| Failure::Other(e.to_string()))
}

fn https_only(url: &str) -> bool {
    url.starts_with("https://") && !url.contains(['"', '\'', '<', '>', ' ', '\\'])
}

fn plain_word(text: &str, dashes: bool) -> bool {
    !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (dashes && c == '-'))
}

fn parse_badges(body: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for set in body["data"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let Some(set_id) = set["set_id"].as_str().filter(|id| plain_word(id, true)) else { continue };
        for version in set["versions"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let Some(id) = version["id"].as_str().filter(|id| plain_word(id, true)) else { continue };
            let picture = version["image_url_2x"].as_str().or_else(|| version["image_url_1x"].as_str());
            if let Some(picture) = picture.filter(|p| https_only(p)) {
                out.push((format!("{set_id}/{id}"), picture.to_string()));
            }
        }
    }
    out
}

fn badge_set(room: &str) -> Option<Vec<(String, String)>> {
    if let Ok(cache) = BADGES.lock() {
        if let Some(found) = cache.as_ref().and_then(|all| all.get(room)) {
            return Some(found.clone());
        }
    }
    if let Ok(failed) = BADGES_FAILED.lock() {
        if failed.as_ref().and_then(|all| all.get(room)).is_some_and(|when| when.elapsed() < RETRY_AFTER) {
            return None;
        }
    }

    let path = if room.is_empty() { String::from("chat/badges/global") } else { format!("chat/badges?broadcaster_id={room}") };
    match helix(&path) {
        Ok(body) => {
            let list = parse_badges(&body);
            if let Ok(mut cache) = BADGES.lock() {
                cache.get_or_insert_with(HashMap::new).insert(room.to_string(), list.clone());
            }
            log_msg(&format!("[neko-overlays-plugin] loaded {} badge pictures ({})", list.len(), if room.is_empty() { "global" } else { "channel" }));
            Some(list)
        }
        Err(Failure::NoLogin) => None,
        Err(Failure::Other(why)) => {
            log_msg(&format!("[neko-overlays-plugin] could not load badges: {why}"));
            if let Ok(mut failed) = BADGES_FAILED.lock() {
                failed.get_or_insert_with(HashMap::new).insert(room.to_string(), Instant::now());
            }
            None
        }
    }
}

pub fn badges(room: &str) -> Value {
    let room = if room.chars().all(|c| c.is_ascii_digit()) && room.len() <= 20 { room } else { "" };
    let mut pictures = Map::new();
    let mut ready = true;
    let rooms: Vec<&str> = if room.is_empty() { vec![""] } else { vec!["", room] };
    for key in rooms {
        match badge_set(key) {
            Some(list) => {
                for (name, url) in list {
                    pictures.insert(name, Value::String(url));
                }
            }
            None => ready = false,
        }
    }
    json!({ "ready": ready, "badges": pictures })
}

pub fn avatars(logins: &[&str]) -> Value {
    let mut wanted: Vec<String> = Vec::new();
    for login in logins {
        let login = login.trim().to_lowercase();
        if login.len() <= 25 && plain_word(&login, false) && !wanted.contains(&login) && wanted.len() < MAX_LOGINS_PER_REQUEST {
            wanted.push(login);
        }
    }

    let missing: Vec<String> = {
        let cache = AVATARS.lock().ok();
        wanted
            .iter()
            .filter(|login| !cache.as_ref().and_then(|c| c.as_ref()).is_some_and(|c| c.contains_key(*login)))
            .cloned()
            .collect()
    };

    let recently_failed = AVATARS_FAILED.lock().ok().and_then(|f| *f).is_some_and(|when| when.elapsed() < RETRY_AFTER);
    if !missing.is_empty() && !recently_failed {
        for chunk in missing.chunks(USERS_PER_CALL) {
            let query: Vec<String> = chunk.iter().map(|login| format!("login={login}")).collect();
            match helix(&format!("users?{}", query.join("&"))) {
                Ok(body) => {
                    let mut found: HashSet<String> = HashSet::new();
                    if let Ok(mut cache) = AVATARS.lock() {
                        let cache = cache.get_or_insert_with(HashMap::new);
                        if cache.len() > MAX_CACHED_AVATARS {
                            cache.clear();
                        }
                        for user in body["data"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                            let login = user["login"].as_str().unwrap_or("").to_lowercase();
                            let picture = user["profile_image_url"].as_str().filter(|p| https_only(p)).unwrap_or("");
                            if !login.is_empty() {
                                cache.insert(login.clone(), picture.to_string());
                                found.insert(login);
                            }
                        }
                        for login in chunk {
                            if !found.contains(login) {
                                cache.insert(login.clone(), String::new());
                            }
                        }
                    }
                }
                Err(Failure::NoLogin) => break,
                Err(Failure::Other(why)) => {
                    log_msg(&format!("[neko-overlays-plugin] could not load profile pictures: {why}"));
                    if let Ok(mut failed) = AVATARS_FAILED.lock() {
                        *failed = Some(Instant::now());
                    }
                    break;
                }
            }
        }
    }

    let mut out = Map::new();
    if let Ok(cache) = AVATARS.lock() {
        if let Some(cache) = cache.as_ref() {
            for login in &wanted {
                if let Some(url) = cache.get(login).filter(|url| !url.is_empty()) {
                    out.insert(login.clone(), Value::String(url.clone()));
                }
            }
        }
    }
    Value::Object(out)
}
