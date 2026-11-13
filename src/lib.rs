#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

mod chat;
mod chat_overlay;
mod fonts;
mod icons;
mod lookups;
mod info_bar;
mod server;
mod twitch;

use ffi::*;

static MODULE_PTR: AtomicPtr<obs_module_t> = AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn obs_module_set_pointer(module: *mut obs_module_t) {
    MODULE_PTR.store(module, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn obs_current_module() -> *mut obs_module_t {
    MODULE_PTR.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn obs_module_ver() -> u32 {
    (LIBOBS_API_MAJOR_VER << 24) | (LIBOBS_API_MINOR_VER << 16) | LIBOBS_API_PATCH_VER
}

#[no_mangle]
pub extern "C" fn obs_module_name() -> *const c_char {
    c"Neko Overlays".as_ptr()
}

#[no_mangle]
pub extern "C" fn obs_module_description() -> *const c_char {
    c"Stream overlays that run on your own computer".as_ptr()
}

#[no_mangle]
pub extern "C" fn obs_module_load() -> bool {
    log_info(c"[neko-overlays-plugin] plugin loaded");
    server::start();
    // icons::install(); // custom source icons crashed OBS 33.0.1, off until the cause is found
    info_bar::register();
    chat_overlay::register();
    twitch::start();
    true
}

#[no_mangle]
pub extern "C" fn obs_module_unload() {
    // The Twitch thread must be gone before OBS unmaps this library.
    twitch::stop();
    server::stop();
    log_info(c"[neko-overlays-plugin] plugin unloaded");
}

pub(crate) fn log_info(msg: &std::ffi::CStr) {
    // blog is printf-style: pass the message as an argument, never as the format.
    unsafe {
        blog(LOG_INFO as i32, c"%s".as_ptr(), msg.as_ptr());
    }
}

pub(crate) fn log_msg(msg: &str) {
    if let Ok(text) = std::ffi::CString::new(msg) {
        log_info(&text);
    }
}

pub(crate) fn config_file(name: &str) -> Option<std::path::PathBuf> {
    let name = std::ffi::CString::new(name).ok()?;
    unsafe {
        let raw = obs_module_get_config_path(obs_current_module(), name.as_ptr());
        if raw.is_null() {
            return None;
        }
        let path = std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned();
        bfree(raw as *mut c_void);
        Some(std::path::PathBuf::from(path))
    }
}

#[allow(unused)]
type Opaque = *mut c_void;