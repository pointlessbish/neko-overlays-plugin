use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

use crate::{config_file, log_msg};

struct Icon {
    file: &'static str,
    svg: &'static str,
}

const CHAT_DARK: Icon = Icon { file: "icons/chat-dark.svg", svg: include_str!("../assets/icons/chat-dark.svg") };
const CHAT_LIGHT: Icon = Icon { file: "icons/chat-light.svg", svg: include_str!("../assets/icons/chat-light.svg") };
const BAR_DARK: Icon = Icon { file: "icons/info-bar-dark.svg", svg: include_str!("../assets/icons/info-bar-dark.svg") };
const BAR_LIGHT: Icon = Icon { file: "icons/info-bar-light.svg", svg: include_str!("../assets/icons/info-bar-light.svg") };

fn write(icon: &Icon) -> Option<CString> {
    let path = config_file(icon.file)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    if let Err(e) = std::fs::write(&path, icon.svg) {
        log_msg(&format!("[neko-overlays-plugin] could not write icon {}: {e}", path.display()));
        return None;
    }
    CString::new(path.to_string_lossy().into_owned()).ok()
}

struct Paths {
    chat_dark: Option<CString>,
    chat_light: Option<CString>,
    bar_dark: Option<CString>,
    bar_light: Option<CString>,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn install() {
    PATHS.get_or_init(|| Paths {
        chat_dark: write(&CHAT_DARK),
        chat_light: write(&CHAT_LIGHT),
        bar_dark: write(&BAR_DARK),
        bar_light: write(&BAR_LIGHT),
    });
}

fn ptr(path: &Option<CString>) -> *const c_char {
    path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr())
}

pub unsafe extern "C" fn chat_dark(_type_data: *mut c_void) -> *const c_char {
    PATHS.get().map_or(std::ptr::null(), |p| ptr(&p.chat_dark))
}
pub unsafe extern "C" fn chat_light(_type_data: *mut c_void) -> *const c_char {
    PATHS.get().map_or(std::ptr::null(), |p| ptr(&p.chat_light))
}
pub unsafe extern "C" fn bar_dark(_type_data: *mut c_void) -> *const c_char {
    PATHS.get().map_or(std::ptr::null(), |p| ptr(&p.bar_dark))
}
pub unsafe extern "C" fn bar_light(_type_data: *mut c_void) -> *const c_char {
    PATHS.get().map_or(std::ptr::null(), |p| ptr(&p.bar_light))
}
