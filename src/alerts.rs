use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

// Kinds an alert can have. The page (assets/alerts.js) knows the same names.
pub const FOLLOW: &str = "follow";
pub const SUB: &str = "sub";
pub const RESUB: &str = "resub";
pub const GIFT: &str = "gift";
pub const MYSTERY_GIFT: &str = "mysterygift";
pub const CHEER: &str = "cheer";
pub const RAID: &str = "raid";

const LOG_LIMIT: usize = 50;

// The chat colour the test alerts pretend their viewer has (styles that paint the name in it, like THE FINALS, show it).
const TEST_COLOR: &str = "#8a5cf5";

struct AlertEvent {
    seq: u64,
    time_ms: u64,
    kind: &'static str,
    name: String,
    amount: Option<u64>,
    tier: String,
    message: String,
    extra: String,
    color: String,
}

static LOG: Mutex<VecDeque<AlertEvent>> = Mutex::new(VecDeque::new());
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Adds an alert. `tier` is Twitch's plan text ("Prime", "1000", "2000", "3000") or empty.
/// `extra` holds the gift recipient for a single gifted sub. `color` is the viewer's Twitch chat colour as "#rrggbb"
/// (empty when unknown); only some styles use it. The page decides what to show.
pub fn push(
    kind: &'static str,
    name: &str,
    amount: Option<u64>,
    tier: &str,
    message: &str,
    extra: &str,
    color: &str,
) {
    let event = AlertEvent {
        seq: NEXT_SEQ.fetch_add(1, Ordering::SeqCst),
        time_ms: now_ms(),
        kind,
        name: name.chars().take(40).collect(),
        amount,
        tier: tier.chars().take(10).collect(),
        message: message.chars().take(500).collect(),
        extra: extra.chars().take(40).collect(),
        color: color.chars().take(7).collect(),
    };
    if let Ok(mut log) = LOG.lock() {
        log.push_back(event);
        while log.len() > LOG_LIMIT {
            log.pop_front();
        }
    }
}

/// Made-up alert for the "Test ..." buttons in the source's properties.
pub fn push_test(kind: &str) {
    match kind {
        FOLLOW => push(FOLLOW, "NekoFan", None, "", "", "", TEST_COLOR),
        SUB => push(
            SUB,
            "NekoFan",
            Some(1),
            "1000",
            "Hello from the test button",
            "",
            TEST_COLOR,
        ),
        RESUB => push(
            RESUB,
            "NekoFan",
            Some(12),
            "2000",
            "A whole year already",
            "",
            TEST_COLOR,
        ),
        GIFT => push(
            GIFT,
            "NekoFan",
            Some(1),
            "1000",
            "",
            "SomeoneNice",
            TEST_COLOR,
        ),
        MYSTERY_GIFT => push(MYSTERY_GIFT, "NekoFan", Some(5), "1000", "", "", TEST_COLOR),
        CHEER => push(
            CHEER,
            "NekoFan",
            Some(500),
            "",
            "Take my bits",
            "",
            TEST_COLOR,
        ),
        RAID => push(RAID, "NekoFan", Some(42), "", "", "", TEST_COLOR),
        _ => {}
    }
}

/// What a custom file is, by its extension: ("image" | "video" | "audio", content type). None = not allowed.
/// Only these types are ever served to the page.
pub fn media_kind(path: &str) -> Option<(&'static str, &'static str)> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    let found = match extension.as_str() {
        "png" => ("image", "image/png"),
        "jpg" | "jpeg" => ("image", "image/jpeg"),
        "gif" => ("image", "image/gif"),
        "webp" => ("image", "image/webp"),
        "apng" => ("image", "image/apng"),
        "avif" => ("image", "image/avif"),
        "svg" => ("image", "image/svg+xml"),
        "mp4" | "m4v" => ("video", "video/mp4"),
        "webm" => ("video", "video/webm"),
        "ogv" => ("video", "video/ogg"),
        "mp3" => ("audio", "audio/mpeg"),
        "ogg" | "oga" => ("audio", "audio/ogg"),
        "wav" => ("audio", "audio/wav"),
        "m4a" => ("audio", "audio/mp4"),
        "aac" => ("audio", "audio/aac"),
        "flac" => ("audio", "audio/flac"),
        _ => return None,
    };
    Some(found)
}

fn event_json(event: &AlertEvent) -> Value {
    let mut value = json!({
        "seq": event.seq,
        "ts": event.time_ms,
        "kind": event.kind,
        "name": event.name,
        "tier": event.tier,
        "message": event.message,
        "extra": event.extra,
        "color": event.color,
    });
    if let Some(amount) = event.amount {
        value["amount"] = json!(amount);
    }
    value
}

pub fn state_json() -> Value {
    let events: Vec<Value> = LOG
        .lock()
        .map(|log| log.iter().map(event_json).collect::<Vec<Value>>())
        .unwrap_or_default();
    json!({ "events": events })
}
