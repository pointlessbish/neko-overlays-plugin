use std::ffi::{c_char, c_void, CString};
use std::ptr;
use std::sync::OnceLock;

use crate::ffi::*;
use crate::{config_file, log_msg};

// The icons shown next to the three sources in OBS (the source list, the "Add source" dialog and the dropdowns). OBS 32.2
// and newer ask the source for the path of an SVG file, one for dark themes and one for light themes, so the files are
// written to the plugin's config folder when the plugin loads.

struct Icon {
    file: &'static str,
    svg: &'static str,
}

const CHAT_DARK: Icon = Icon {
    file: "icons/chat-dark.svg",
    svg: include_str!("../assets/icons/chat-dark.svg"),
};
const CHAT_LIGHT: Icon = Icon {
    file: "icons/chat-light.svg",
    svg: include_str!("../assets/icons/chat-light.svg"),
};
const BAR_DARK: Icon = Icon {
    file: "icons/info-bar-dark.svg",
    svg: include_str!("../assets/icons/info-bar-dark.svg"),
};
const BAR_LIGHT: Icon = Icon {
    file: "icons/info-bar-light.svg",
    svg: include_str!("../assets/icons/info-bar-light.svg"),
};
const ALERTS_DARK: Icon = Icon {
    file: "icons/alerts-dark.svg",
    svg: include_str!("../assets/icons/alerts-dark.svg"),
};
const ALERTS_LIGHT: Icon = Icon {
    file: "icons/alerts-light.svg",
    svg: include_str!("../assets/icons/alerts-light.svg"),
};

// OBS_ICON_TYPE_CUSTOM in libobs's enum obs_icon_type (unknown, image, color, slideshow, audio input, audio output, desktop
// capture, window capture, game capture, camera, text, media, browser, custom): 13.
// bindgen types C enums as u32 on Linux/macOS but i32 on Windows (MSVC), so the assignment below casts with `as _`.
const ICON_TYPE_CUSTOM: u32 = 13;

fn write(icon: &Icon) -> Option<CString> {
    let path = config_file(icon.file)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    if let Err(e) = std::fs::write(&path, icon.svg) {
        log_msg(&format!(
            "[neko-overlays-plugin] could not write icon {}: {e}",
            path.display()
        ));
        return None;
    }
    CString::new(path.to_string_lossy().into_owned()).ok()
}

struct Paths {
    chat_dark: Option<CString>,
    chat_light: Option<CString>,
    bar_dark: Option<CString>,
    bar_light: Option<CString>,
    alerts_dark: Option<CString>,
    alerts_light: Option<CString>,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn install() {
    PATHS.get_or_init(|| Paths {
        chat_dark: write(&CHAT_DARK),
        chat_light: write(&CHAT_LIGHT),
        bar_dark: write(&BAR_DARK),
        bar_light: write(&BAR_LIGHT),
        alerts_dark: write(&ALERTS_DARK),
        alerts_light: write(&ALERTS_LIGHT),
    });
}

// OBS frees the path it gets back from these callbacks (the frontend calls bfree on it after loading the icon), so the
// path has to be a fresh copy made by OBS's own allocator every time (bmalloc; bstrdup is an inline helper in OBS's headers,
// so it has no binding). Handing back a pointer to our own memory makes OBS free memory it does not own, which crashed
// OBS 33.0.1 when the icons were first tried. No file (null) = OBS's default icon.
unsafe fn copy_for_obs(path: &Option<CString>) -> *const c_char {
    let Some(path) = path else { return ptr::null() };
    let bytes = path.as_bytes_with_nul();
    let copy = bmalloc(bytes.len()) as *mut u8;
    if copy.is_null() {
        return ptr::null();
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), copy, bytes.len());
    copy as *const c_char
}

macro_rules! icon_callback {
    ($name:ident, $field:ident) => {
        unsafe extern "C" fn $name(_type_data: *mut c_void) -> *const c_char {
            PATHS
                .get()
                .map_or(ptr::null(), |paths| copy_for_obs(&paths.$field))
        }
    };
}

icon_callback!(chat_dark, chat_dark);
icon_callback!(chat_light, chat_light);
icon_callback!(bar_dark, bar_dark);
icon_callback!(bar_light, bar_light);
icon_callback!(alerts_dark, alerts_dark);
icon_callback!(alerts_light, alerts_light);

type IconCallback = unsafe extern "C" fn(*mut c_void) -> *const c_char;

pub enum Kind {
    Chat,
    Bar,
    Alerts,
}

/// Gives a source's definition its custom icon. Call before obs_register_source_s.
pub fn apply(info: &mut obs_source_info, kind: Kind) {
    let (dark, light): (IconCallback, IconCallback) = match kind {
        Kind::Chat => (chat_dark, chat_light),
        Kind::Bar => (bar_dark, bar_light),
        Kind::Alerts => (alerts_dark, alerts_light),
    };
    info.icon_type = ICON_TYPE_CUSTOM as _;
    info.get_dark_icon = Some(dark);
    info.get_light_icon = Some(light);
}
