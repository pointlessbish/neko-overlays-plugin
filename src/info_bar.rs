// Location: neko-overlays-plugin/src/info_bar.rs
// File: info_bar.rs

//! "Neko Info Bar": the rotating info bar with all of the overlay styles, live Twitch data and the
//! subscriber train.
//!
//! The bar itself is a web page that the plugin serves on this computer (server.rs). The source you add
//! in OBS wraps an OBS browser source that shows that page, and turns the properties you see here into
//! the settings the page uses. The page loads nothing from the internet; Google fonts are downloaded once by
//! the plugin (fonts.rs) and then served from this computer, unless "Download Google Fonts" is switched off.
//!
//! This needs the browser source that comes with OBS (the Flatpak version has it).

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use serde_json::{json, Map, Value};

use crate::ffi::*;
use crate::log_info;
use crate::server::{self, THEMES};
use crate::twitch;

// Colors are ABGR (0xAABBGGRR), as used by OBS color properties. These match the "Classic" style.
const DEFAULT_BACKGROUND: i64 = 0xFF14_0026; // #260014
const DEFAULT_ACCENT: i64 = 0xFFC0_A6F1; // #f1a6c0
const DEFAULT_TEXT: i64 = 0xFFFC_FFFD; // #fdfffc

const BROWSER_CSS: &CStr = c"body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }";

struct InfoBar {
    /// The private OBS browser source that shows the page.
    browser: *mut obs_source_t,
    /// The id this bar's page is served under (server.rs).
    id: u32,
    width: u32,
    height: u32,
    /// What the browser source was last given, so it is only touched (and reloaded) when something changed.
    applied: Option<(String, u32, u32)>,
}

// --- Settings -------------------------------------------------------------------------------

unsafe fn get_str(settings: *mut obs_data_t, name: &CStr) -> String {
    CStr::from_ptr(obs_data_get_string(settings, name.as_ptr()))
        .to_string_lossy()
        .into_owned()
}

unsafe fn get_bool(settings: *mut obs_data_t, name: &CStr) -> bool {
    obs_data_get_bool(settings, name.as_ptr())
}

unsafe fn get_int(settings: *mut obs_data_t, name: &CStr, min: i64, max: i64) -> i64 {
    obs_data_get_int(settings, name.as_ptr()).clamp(min, max)
}

/// 0xAABBGGRR -> "#rrggbb"
fn hex(color: i64) -> String {
    format!("#{:02x}{:02x}{:02x}", color & 0xff, (color >> 8) & 0xff, (color >> 16) & 0xff)
}

/// A font name is only used if it is plain letters, digits and spaces (it ends up in the page's CSS).
fn clean_font(name: &str) -> Option<String> {
    let name = name.trim();
    let plain = !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
    plain.then(|| name.to_string())
}

/// The properties as the settings object the page understands.
unsafe fn page_settings(settings: *mut obs_data_t) -> Value {
    let theme = get_str(settings, c"theme");
    let theme = if THEMES.iter().any(|(id, _)| *id == theme) { theme } else { String::from("classic") };

    let mut colors = Map::new();
    if get_bool(settings, c"custom_colors") {
        colors.insert("accent".into(), json!(hex(obs_data_get_int(settings, c"accent_color".as_ptr()))));
        colors.insert("background".into(), json!(hex(obs_data_get_int(settings, c"background_color".as_ptr()))));
        colors.insert("text".into(), json!(hex(obs_data_get_int(settings, c"text_color".as_ptr()))));
        colors.insert("textAccent".into(), json!(hex(obs_data_get_int(settings, c"text_accent_color".as_ptr()))));
    }

    let font_size = get_int(settings, c"font_size", 0, 96);
    let font_size = if font_size > 0 { json!(font_size) } else { Value::Null };
    let train_type = if get_str(settings, c"train_type") == "follower" { "follower" } else { "subscriber" };

    json!({
        "theme": theme,
        "colors": colors,
        "font": clean_font(&get_str(settings, c"font")),
        "fontSize": font_size,
        "googleFonts": get_bool(settings, c"google_fonts"),
        "rotateSeconds": get_int(settings, c"rotate_seconds", 2, 120),
        "train": {
            "enabled": get_bool(settings, c"train_enabled"),
            "type": train_type,
            "seconds": get_int(settings, c"train_seconds", 5, 600),
        },
        "stats": {
            "followers": get_bool(settings, c"show_followers"),
            "subs": get_bool(settings, c"show_subs"),
            "viewers": get_bool(settings, c"show_viewers"),
        },
        "latest": {
            "follower": get_bool(settings, c"show_latest_follower"),
            "subscriber": get_bool(settings, c"show_latest_subscriber"),
            "cheer": get_bool(settings, c"show_latest_cheer"),
            "streaker": get_bool(settings, c"show_latest_streaker"),
        },
    })
}

/// Points the browser source at this bar's page, and gives it its size. Only does something if that changed.
unsafe fn apply_browser(bar: &mut InfoBar) {
    let port = server::port();
    if port == 0 {
        return; // the local server did not start; the log says why
    }
    let url = server::bar_url(bar.id);
    let wanted = (url.clone(), bar.width, bar.height);
    if bar.applied.as_ref() == Some(&wanted) {
        return;
    }
    let url = CString::new(url).unwrap_or_default();

    let settings = obs_data_create();
    obs_data_set_string(settings, c"url".as_ptr(), url.as_ptr());
    obs_data_set_bool(settings, c"is_local_file".as_ptr(), false);
    obs_data_set_int(settings, c"width".as_ptr(), bar.width as i64);
    obs_data_set_int(settings, c"height".as_ptr(), bar.height as i64);
    obs_data_set_bool(settings, c"shutdown".as_ptr(), false);
    obs_data_set_bool(settings, c"restart_when_active".as_ptr(), false);
    obs_data_set_bool(settings, c"reroute_audio".as_ptr(), false);
    obs_data_set_bool(settings, c"fps_custom".as_ptr(), false);
    obs_data_set_string(settings, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    obs_source_update(bar.browser, settings);
    obs_data_release(settings);

    bar.applied = Some(wanted);
}

// --- Source callbacks ----------------------------------------------------------

unsafe extern "C" fn bar_get_name(_type_data: *mut c_void) -> *const c_char {
    c"Neko Info Bar".as_ptr()
}

unsafe extern "C" fn bar_get_defaults(settings: *mut obs_data_t) {
    obs_data_set_default_string(settings, c"client_id".as_ptr(), c"".as_ptr());

    obs_data_set_default_string(settings, c"theme".as_ptr(), c"classic".as_ptr());
    obs_data_set_default_bool(settings, c"custom_colors".as_ptr(), false);
    obs_data_set_default_int(settings, c"accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_int(settings, c"background_color".as_ptr(), DEFAULT_BACKGROUND);
    obs_data_set_default_int(settings, c"text_color".as_ptr(), DEFAULT_TEXT);
    obs_data_set_default_int(settings, c"text_accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_string(settings, c"font".as_ptr(), c"".as_ptr());
    obs_data_set_default_int(settings, c"font_size".as_ptr(), 0);
    obs_data_set_default_bool(settings, c"google_fonts".as_ptr(), true);

    obs_data_set_default_int(settings, c"rotate_seconds".as_ptr(), 10);
    obs_data_set_default_bool(settings, c"show_followers".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_subs".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_viewers".as_ptr(), false);
    obs_data_set_default_bool(settings, c"show_latest_follower".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_latest_subscriber".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_latest_cheer".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_latest_streaker".as_ptr(), true);

    obs_data_set_default_bool(settings, c"train_enabled".as_ptr(), true);
    obs_data_set_default_string(settings, c"train_type".as_ptr(), c"subscriber".as_ptr());
    obs_data_set_default_int(settings, c"train_seconds".as_ptr(), 30);

    obs_data_set_default_int(settings, c"width".as_ptr(), 1000);
    obs_data_set_default_int(settings, c"height".as_ptr(), 56);
}

unsafe extern "C" fn on_connect(
    _props: *mut obs_properties_t,
    _property: *mut obs_property_t,
    _data: *mut c_void,
) -> bool {
    log_info(c"[neko-overlays-plugin] Connect to Twitch clicked");
    twitch::request_login();
    false
}

unsafe extern "C" fn on_disconnect(
    _props: *mut obs_properties_t,
    _property: *mut obs_property_t,
    _data: *mut c_void,
) -> bool {
    twitch::request_logout();
    false
}

unsafe extern "C" fn bar_get_properties(_data: *mut c_void) -> *mut obs_properties_t {
    let props = obs_properties_create();

    obs_properties_add_button(props, c"connect".as_ptr(), c"Connect to Twitch".as_ptr(), Some(on_connect));
    obs_properties_add_button(props, c"disconnect".as_ptr(), c"Disconnect from Twitch".as_ptr(), Some(on_disconnect));
    // 0 = OBS_TEXT_DEFAULT
    obs_properties_add_text(props, c"client_id".as_ptr(), c"Twitch Client ID (empty = built-in)".as_ptr(), 0);

    // Style. 2 = OBS_COMBO_TYPE_LIST, 3 = OBS_COMBO_FORMAT_STRING
    let theme = obs_properties_add_list(props, c"theme".as_ptr(), c"Style".as_ptr(), 2, 3);
    for (id, label) in THEMES {
        let (id, label) = (CString::new(id).unwrap_or_default(), CString::new(label).unwrap_or_default());
        obs_property_list_add_string(theme, label.as_ptr(), id.as_ptr());
    }
    obs_properties_add_bool(props, c"custom_colors".as_ptr(), c"Use my own colors (otherwise the style's own)".as_ptr());
    obs_properties_add_color(props, c"accent_color".as_ptr(), c"Accent (dividers, borders, glow)".as_ptr());
    obs_properties_add_color(props, c"background_color".as_ptr(), c"Background".as_ptr());
    obs_properties_add_color(props, c"text_color".as_ptr(), c"Text".as_ptr());
    obs_properties_add_color(props, c"text_accent_color".as_ptr(), c"Highlighted names and numbers".as_ptr());
    obs_properties_add_text(props, c"font".as_ptr(), c"Font (empty = the style's own)".as_ptr(), 0);
    obs_properties_add_int(props, c"font_size".as_ptr(), c"Font size in px (0 = the style's own)".as_ptr(), 0, 96, 1);
    obs_properties_add_bool(props, c"google_fonts".as_ptr(), c"Download Google Fonts when a style or the Font box needs one (once)".as_ptr());

    // What the bar shows
    obs_properties_add_int(props, c"rotate_seconds".as_ptr(), c"Seconds per item".as_ptr(), 2, 120, 1);
    obs_properties_add_bool(props, c"show_followers".as_ptr(), c"Number: followers".as_ptr());
    obs_properties_add_bool(props, c"show_subs".as_ptr(), c"Number: subscribers".as_ptr());
    obs_properties_add_bool(props, c"show_viewers".as_ptr(), c"Number: viewers".as_ptr());
    obs_properties_add_bool(props, c"show_latest_follower".as_ptr(), c"Latest follower".as_ptr());
    obs_properties_add_bool(props, c"show_latest_subscriber".as_ptr(), c"Latest subscriber".as_ptr());
    obs_properties_add_bool(props, c"show_latest_cheer".as_ptr(), c"Latest cheer".as_ptr());
    obs_properties_add_bool(props, c"show_latest_streaker".as_ptr(), c"Latest watch streak".as_ptr());

    // The train
    obs_properties_add_bool(props, c"train_enabled".as_ptr(), c"Train bar".as_ptr());
    let train = obs_properties_add_list(props, c"train_type".as_ptr(), c"Train counts".as_ptr(), 2, 3);
    obs_property_list_add_string(train, c"Subscribers".as_ptr(), c"subscriber".as_ptr());
    obs_property_list_add_string(train, c"Followers".as_ptr(), c"follower".as_ptr());
    obs_properties_add_int(props, c"train_seconds".as_ptr(), c"Train seconds".as_ptr(), 5, 600, 1);

    // Size of the source
    obs_properties_add_int(props, c"width".as_ptr(), c"Width".as_ptr(), 100, 4000, 1);
    obs_properties_add_int(props, c"height".as_ptr(), c"Height".as_ptr(), 20, 1000, 1);
    props
}

unsafe extern "C" fn bar_create(settings: *mut obs_data_t, _source: *mut obs_source_t) -> *mut c_void {
    // about:blank: nothing is loaded until the first update points it at this computer's page.
    let start = obs_data_create();
    obs_data_set_string(start, c"url".as_ptr(), c"about:blank".as_ptr());
    obs_data_set_int(start, c"width".as_ptr(), 1000);
    obs_data_set_int(start, c"height".as_ptr(), 56);
    obs_data_set_string(start, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    let browser = obs_source_create_private(c"browser_source".as_ptr(), c"neko-info-bar-browser".as_ptr(), start);
    obs_data_release(start);

    if browser.is_null() {
        log_info(c"[neko-overlays-plugin] this OBS has no browser source, so the info bar cannot be shown (the Flatpak version of OBS has it)");
        return ptr::null_mut();
    }

    let bar = Box::into_raw(Box::new(InfoBar {
        browser,
        id: server::register_bar(),
        width: 1000,
        height: 56,
        applied: None,
    }));
    bar_update(bar as *mut c_void, settings);
    bar as *mut c_void
}

unsafe extern "C" fn bar_destroy(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let bar = Box::from_raw(data as *mut InfoBar);
    server::remove_bar(bar.id);
    obs_source_release(bar.browser);
}

unsafe extern "C" fn bar_update(data: *mut c_void, settings: *mut obs_data_t) {
    let bar = &mut *(data as *mut InfoBar);

    twitch::set_client_id(get_str(settings, c"client_id").trim());

    bar.width = get_int(settings, c"width", 100, 4000) as u32;
    bar.height = get_int(settings, c"height", 20, 1000) as u32;

    server::update_bar(bar.id, page_settings(settings));
    apply_browser(bar);
}

// The browser source has to be told when the bar is shown, or it does not paint.
unsafe extern "C" fn bar_show(data: *mut c_void) {
    obs_source_inc_showing((*(data as *mut InfoBar)).browser);
}

unsafe extern "C" fn bar_hide(data: *mut c_void) {
    obs_source_dec_showing((*(data as *mut InfoBar)).browser);
}

unsafe extern "C" fn bar_activate(data: *mut c_void) {
    obs_source_inc_active((*(data as *mut InfoBar)).browser);
}

unsafe extern "C" fn bar_deactivate(data: *mut c_void) {
    obs_source_dec_active((*(data as *mut InfoBar)).browser);
}

unsafe extern "C" fn bar_video_render(data: *mut c_void, _effect: *mut gs_effect_t) {
    obs_source_video_render((*(data as *mut InfoBar)).browser);
}

unsafe extern "C" fn bar_get_width(data: *mut c_void) -> u32 {
    (*(data as *mut InfoBar)).width
}

unsafe extern "C" fn bar_get_height(data: *mut c_void) -> u32 {
    (*(data as *mut InfoBar)).height
}

// --- Registration ------------------------------------------------------------------

pub fn register() {
    let mut info: obs_source_info = unsafe { std::mem::zeroed() };
    info.id = c"neko_info_bar".as_ptr();
    info.type_ = 0; // OBS_SOURCE_TYPE_INPUT
    // CUSTOM_DRAW: this source only forwards to the browser source, which draws itself. Without the flag OBS wraps our
    // render in its own effect loop, the browser source then starts a second one, and every frame fails with
    // "gs_effect_loop: An effect is already active" and nothing is drawn.
    info.output_flags = (OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW) as u32;
    info.get_name = Some(bar_get_name);
    info.create = Some(bar_create);
    info.destroy = Some(bar_destroy);
    info.update = Some(bar_update);
    info.get_defaults = Some(bar_get_defaults);
    info.get_properties = Some(bar_get_properties);
    info.show = Some(bar_show);
    info.hide = Some(bar_hide);
    info.activate = Some(bar_activate);
    info.deactivate = Some(bar_deactivate);
    info.video_render = Some(bar_video_render);
    info.get_width = Some(bar_get_width);
    info.get_height = Some(bar_get_height);

    unsafe {
        obs_register_source_s(&info, std::mem::size_of::<obs_source_info>());
    }
    log_info(c"[neko-overlays-plugin] registered source 'Neko Info Bar'");
}