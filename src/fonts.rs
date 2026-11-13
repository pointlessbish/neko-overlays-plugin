use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::{config_file, log_msg};

#[derive(Clone, Copy)]
pub enum Status {
    Loading,
    Ready,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Loading => "loading",
            Status::Ready => "ready",
            Status::Failed => "failed",
        }
    }
}

enum Job {
    Loading,
    Failed(Instant),
}

static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
static GENERATION: AtomicU32 = AtomicU32::new(0);

const RETRY_AFTER: Duration = Duration::from_secs(60);
const MAX_FILE_BYTES: u64 = 4_000_000;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const WEIGHT_TRIES: [&str; 3] = [":wght@400;500;600;700;800", ":wght@400;700", ""];

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn generation() -> u32 {
    GENERATION.load(Ordering::SeqCst)
}

pub fn clean_family(name: &str) -> Option<String> {
    let name = name.trim();
    let plain = !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
    plain.then(|| name.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn slug(family: &str) -> String {
    family.to_ascii_lowercase().replace(' ', "-")
}

fn css_name(family: &str) -> String {
    format!("fonts/{}.css", slug(family))
}

fn is_cached(family: &str) -> bool {
    config_file(&css_name(family)).is_some_and(|path| path.is_file())
}

pub fn request(family: &str) -> Status {
    let Some(family) = clean_family(family) else {
        return Status::Failed;
    };
    if is_cached(&family) {
        return Status::Ready;
    }

    let Ok(mut all) = jobs().lock() else {
        return Status::Failed;
    };
    match all.get(&family) {
        Some(Job::Loading) => return Status::Loading,
        Some(Job::Failed(when)) if when.elapsed() < RETRY_AFTER => return Status::Failed,
        _ => {}
    }
    all.insert(family.clone(), Job::Loading);
    drop(all);

    let name = family.clone();
    let spawned = thread::Builder::new().name("neko-fonts".into()).spawn(move || {
        let outcome = download(&name);
        if let Ok(mut all) = jobs().lock() {
            match &outcome {
                Ok(()) => {
                    all.remove(&name);
                    GENERATION.fetch_add(1, Ordering::SeqCst);
                    log_msg(&format!("[neko-overlays-plugin] downloaded Google font \"{name}\""));
                }
                Err(why) => {
                    all.insert(name.clone(), Job::Failed(Instant::now()));
                    log_msg(&format!("[neko-overlays-plugin] could not get Google font \"{name}\": {why}"));
                }
            }
        }
    });
    if spawned.is_err() {
        if let Ok(mut all) = jobs().lock() {
            all.insert(family, Job::Failed(Instant::now()));
        }
        return Status::Failed;
    }
    Status::Loading
}

pub fn combined_css() -> Vec<u8> {
    let Some(folder) = config_file("fonts") else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "css"))
        .collect();
    files.sort();

    let mut css = Vec::new();
    for path in files {
        if let Ok(bytes) = std::fs::read(&path) {
            css.extend_from_slice(&bytes);
            css.push(b'\n');
        }
    }
    css
}

fn fetch(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = agent
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            other => other.to_string(),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn file_stem(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn download(family: &str) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
    let plus = family.replace(' ', "+");

    let mut sheet = None;
    let mut last_error = String::new();
    for weights in WEIGHT_TRIES {
        let url = format!("https://fonts.googleapis.com/css2?family={plus}{weights}&display=swap");
        match fetch(&agent, &url, 500_000) {
            Ok(bytes) => {
                sheet = Some(String::from_utf8_lossy(&bytes).into_owned());
                break;
            }
            Err(why) => last_error = why,
        }
    }
    let mut css = sheet.ok_or_else(|| format!("no such family on Google Fonts ({last_error})"))?;
    if !css.contains("@font-face") {
        return Err("Google sent no font".into());
    }

    const OPEN: &str = "url(";
    const HOST: &str = "https://fonts.gstatic.com/";
    let mut urls: Vec<String> = Vec::new();
    let mut rest = css.as_str();
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(')') else { break };
        let url = after[..end].trim_matches(|c| c == '"' || c == '\'');
        if url.starts_with(HOST) && !urls.iter().any(|known| known == url) {
            urls.push(url.to_string());
        }
        rest = &after[end..];
    }
    if urls.is_empty() {
        return Err("Google sent no font files".into());
    }

    for url in &urls {
        let extension = url.rsplit('.').next().filter(|ext| matches!(*ext, "woff2" | "woff" | "ttf" | "otf")).unwrap_or("woff2");
        let name = format!("{}.{extension}", file_stem(url));
        let path = config_file(&format!("fonts/{name}")).ok_or("no config folder")?;
        if !path.is_file() {
            let bytes = fetch(&agent, url, MAX_FILE_BYTES)?;
            if bytes.is_empty() {
                return Err("an empty font file".into());
            }
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
            }
            let partial = path.with_extension("part");
            std::fs::write(&partial, &bytes).map_err(|error| error.to_string())?;
            std::fs::rename(&partial, &path).map_err(|error| error.to_string())?;
        }
        css = css.replace(url.as_str(), &format!("/fonts/{name}"));
    }

    let path = config_file(&css_name(family)).ok_or("no config folder")?;
    let partial = path.with_extension("part");
    std::fs::write(&partial, css).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, &path).map_err(|error| error.to_string())?;
    Ok(())
}