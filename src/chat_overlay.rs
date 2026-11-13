use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use serde_json::{json, Map, Value};

use crate::ffi::*;
use crate::log_info;
use crate::server::{self, THEMES};
use crate::twitch;

const DEFAULT_BACKGROUND: i64 = 0xFF14_0026;
const DEFAULT_ACCENT: i64 = 0xFFC0_A6F1;
const DEFAULT_TEXT: i64 = 0xFFFC_FFFD;

const DEFAULT_BLOCKLIST: &CStr = c"streamelements\nstreamlabs";

const BROWSER_CSS: &CStr = c"body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }";

struct ChatOverlay {
    browser: *mut obs_source_t,
    id: u32,
    width: u32,
    height: u32,
    applied: Option<(String, u32, u32)>,
}

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

fn hex(color: i64) -> String {
    format!("#{:02x}{:02x}{:02x}", color & 0xff, (color >> 8) & 0xff, (color >> 16) & 0xff)
}

fn clean_font(name: &str) -> Option<String> {
    let name = name.trim();
    let plain = !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
    plain.then(|| name.to_string())
}

fn blocklist(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let name = word.trim().trim_start_matches('@').to_lowercase();
        let valid = !name.is_empty() && name.len() <= 25 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if valid && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

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

    json!({
        "theme": theme,
        "colors": colors,
        "font": clean_font(&get_str(settings, c"font")),
        "fontSize": font_size,
        "googleFonts": get_bool(settings, c"google_fonts"),
        "maxMessages": get_int(settings, c"max_messages", 1, 50),
        "lifetimeSeconds": get_int(settings, c"lifetime_seconds", 0, 3600),
        "twitchColors": get_bool(settings, c"twitch_colors"),
        "badges": get_bool(settings, c"show_badges"),
        "avatars": get_bool(settings, c"show_avatars"),
        "thirdParty": get_bool(settings, c"third_party_emotes"),
        "blocklist": blocklist(&get_str(settings, c"blocklist")),
    })
}

unsafe fn apply_browser(chat: &mut ChatOverlay) {
    let port = server::port();
    if port == 0 {
        return;
    }
    let url = server::chat_url(chat.id);
    let wanted = (url.clone(), chat.width, chat.height);
    if chat.applied.as_ref() == Some(&wanted) {
        return;
    }
    let url = CString::new(url).unwrap_or_default();

    let settings = obs_data_create();
    obs_data_set_string(settings, c"url".as_ptr(), url.as_ptr());
    obs_data_set_bool(settings, c"is_local_file".as_ptr(), false);
    obs_data_set_int(settings, c"width".as_ptr(), chat.width as i64);
    obs_data_set_int(settings, c"height".as_ptr(), chat.height as i64);
    obs_data_set_bool(settings, c"shutdown".as_ptr(), false);
    obs_data_set_bool(settings, c"restart_when_active".as_ptr(), false);
    obs_data_set_bool(settings, c"reroute_audio".as_ptr(), false);
    obs_data_set_bool(settings, c"fps_custom".as_ptr(), false);
    obs_data_set_string(settings, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    obs_source_update(chat.browser, settings);
    obs_data_release(settings);

    chat.applied = Some(wanted);
}

unsafe extern "C" fn chat_get_name(_type_data: *mut c_void) -> *const c_char {
    c"Neko Chat".as_ptr()
}

unsafe extern "C" fn chat_get_defaults(settings: *mut obs_data_t) {
    obs_data_set_default_string(settings, c"theme".as_ptr(), c"classic".as_ptr());
    obs_data_set_default_bool(settings, c"custom_colors".as_ptr(), false);
    obs_data_set_default_int(settings, c"accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_int(settings, c"background_color".as_ptr(), DEFAULT_BACKGROUND);
    obs_data_set_default_int(settings, c"text_color".as_ptr(), DEFAULT_TEXT);
    obs_data_set_default_int(settings, c"text_accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_string(settings, c"font".as_ptr(), c"".as_ptr());
    obs_data_set_default_int(settings, c"font_size".as_ptr(), 0);
    obs_data_set_default_bool(settings, c"google_fonts".as_ptr(), true);

    obs_data_set_default_int(settings, c"max_messages".as_ptr(), 12);
    obs_data_set_default_int(settings, c"lifetime_seconds".as_ptr(), 0);
    obs_data_set_default_bool(settings, c"twitch_colors".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_badges".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_avatars".as_ptr(), true);
    obs_data_set_default_bool(settings, c"third_party_emotes".as_ptr(), true);
    obs_data_set_default_string(settings, c"blocklist".as_ptr(), DEFAULT_BLOCKLIST.as_ptr());

    obs_data_set_default_int(settings, c"width".as_ptr(), 420);
    obs_data_set_default_int(settings, c"height".as_ptr(), 600);
}

unsafe extern "C" fn on_connect(
    _props: *mut obs_properties_t,
    _property: *mut obs_property_t,
    _data: *mut c_void,
) -> bool {
    log_info(c"[neko-overlays-plugin] Connect to Twitch clicked");
    twitch::request_login();
    crate::server::open_connect_page();
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

unsafe extern "C" fn chat_get_properties(_data: *mut c_void) -> *mut obs_properties_t {
    let props = obs_properties_create();

    obs_properties_add_button(props, c"connect".as_ptr(), c"Connect to Twitch".as_ptr(), Some(on_connect));
    obs_properties_add_button(props, c"disconnect".as_ptr(), c"Disconnect from Twitch".as_ptr(), Some(on_disconnect));

    let theme = obs_properties_add_list(props, c"theme".as_ptr(), c"Style".as_ptr(), 2, 3);
    for (id, label) in THEMES {
        let (id, label) = (CString::new(id).unwrap_or_default(), CString::new(label).unwrap_or_default());
        obs_property_list_add_string(theme, label.as_ptr(), id.as_ptr());
    }
    obs_properties_add_bool(props, c"custom_colors".as_ptr(), c"Use my own colors (otherwise the style's own)".as_ptr());
    obs_properties_add_color(props, c"accent_color".as_ptr(), c"Accent (borders, glow)".as_ptr());
    obs_properties_add_color(props, c"background_color".as_ptr(), c"Background".as_ptr());
    obs_properties_add_color(props, c"text_color".as_ptr(), c"Text".as_ptr());
    obs_properties_add_color(props, c"text_accent_color".as_ptr(), c"Names (when Twitch name colors are off)".as_ptr());
    obs_properties_add_text(props, c"font".as_ptr(), c"Font (empty = the style's own)".as_ptr(), 0);
    obs_properties_add_int(props, c"font_size".as_ptr(), c"Font size in px (0 = the style's own)".as_ptr(), 0, 96, 1);
    obs_properties_add_bool(props, c"google_fonts".as_ptr(), c"Download Google Fonts when a style or the Font box needs one (once)".as_ptr());

    obs_properties_add_int(props, c"max_messages".as_ptr(), c"Messages on screen".as_ptr(), 1, 50, 1);
    obs_properties_add_int(props, c"lifetime_seconds".as_ptr(), c"Hide a message after this many seconds (0 = keep it until it is pushed out)".as_ptr(), 0, 3600, 1);
    obs_properties_add_bool(props, c"twitch_colors".as_ptr(), c"Use the viewers' Twitch name colors".as_ptr());
    obs_properties_add_bool(props, c"show_badges".as_ptr(), c"Show badges".as_ptr());
    obs_properties_add_bool(props, c"show_avatars".as_ptr(), c"Show profile pictures".as_ptr());
    obs_properties_add_bool(props, c"third_party_emotes".as_ptr(), c"Also show 7TV, BTTV and FrankerFaceZ emotes (Twitch emotes always show)".as_ptr());
    obs_properties_add_text(props, c"blocklist".as_ptr(), c"Hidden users, one name per line (bots)".as_ptr(), 2);

    obs_properties_add_int(props, c"width".as_ptr(), c"Width".as_ptr(), 100, 4000, 1);
    obs_properties_add_int(props, c"height".as_ptr(), c"Height".as_ptr(), 20, 4000, 1);
    props
}

unsafe extern "C" fn chat_create(settings: *mut obs_data_t, _source: *mut obs_source_t) -> *mut c_void {
    let start = obs_data_create();
    obs_data_set_string(start, c"url".as_ptr(), c"about:blank".as_ptr());
    obs_data_set_int(start, c"width".as_ptr(), 420);
    obs_data_set_int(start, c"height".as_ptr(), 600);
    obs_data_set_string(start, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    let browser = obs_source_create_private(c"browser_source".as_ptr(), c"neko-chat-browser".as_ptr(), start);
    obs_data_release(start);

    if browser.is_null() {
        log_info(c"[neko-overlays-plugin] this OBS has no browser source, so the chat overlay cannot be shown (the Flatpak version of OBS has it)");
        return ptr::null_mut();
    }

    let chat = Box::into_raw(Box::new(ChatOverlay {
        browser,
        id: server::register_bar(),
        width: 420,
        height: 600,
        applied: None,
    }));
    chat_update(chat as *mut c_void, settings);
    chat as *mut c_void
}

unsafe extern "C" fn chat_destroy(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let chat = Box::from_raw(data as *mut ChatOverlay);
    server::remove_bar(chat.id);
    obs_source_release(chat.browser);
}

unsafe extern "C" fn chat_update(data: *mut c_void, settings: *mut obs_data_t) {
    let chat = &mut *(data as *mut ChatOverlay);

    chat.width = get_int(settings, c"width", 100, 4000) as u32;
    chat.height = get_int(settings, c"height", 20, 4000) as u32;

    server::update_bar(chat.id, page_settings(settings));
    apply_browser(chat);
}

// The browser source must be told when it is shown, or it does not paint.
unsafe extern "C" fn chat_show(data: *mut c_void) {
    obs_source_inc_showing((*(data as *mut ChatOverlay)).browser);
}

unsafe extern "C" fn chat_hide(data: *mut c_void) {
    obs_source_dec_showing((*(data as *mut ChatOverlay)).browser);
}

unsafe extern "C" fn chat_activate(data: *mut c_void) {
    obs_source_inc_active((*(data as *mut ChatOverlay)).browser);
}

unsafe extern "C" fn chat_deactivate(data: *mut c_void) {
    obs_source_dec_active((*(data as *mut ChatOverlay)).browser);
}

unsafe extern "C" fn chat_video_render(data: *mut c_void, _effect: *mut gs_effect_t) {
    obs_source_video_render((*(data as *mut ChatOverlay)).browser);
}

unsafe extern "C" fn chat_get_width(data: *mut c_void) -> u32 {
    (*(data as *mut ChatOverlay)).width
}

unsafe extern "C" fn chat_get_height(data: *mut c_void) -> u32 {
    (*(data as *mut ChatOverlay)).height
}

pub fn register() {
    let mut info: obs_source_info = unsafe { std::mem::zeroed() };
    info.id = c"neko_chat".as_ptr();
    info.type_ = 0;
    // CUSTOM_DRAW: the browser source draws itself; without the flag every frame fails with "gs_effect_loop: An effect is already active".
    info.output_flags = (OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW) as u32;
    info.get_name = Some(chat_get_name);
    info.create = Some(chat_create);
    info.destroy = Some(chat_destroy);
    info.update = Some(chat_update);
    info.get_defaults = Some(chat_get_defaults);
    info.get_properties = Some(chat_get_properties);
    info.show = Some(chat_show);
    info.hide = Some(chat_hide);
    info.activate = Some(chat_activate);
    info.deactivate = Some(chat_deactivate);
    info.video_render = Some(chat_video_render);
    info.get_width = Some(chat_get_width);
    info.get_height = Some(chat_get_height);

    unsafe {
        obs_register_source_s(&info, std::mem::size_of::<obs_source_info>());
    }
    log_info(c"[neko-overlays-plugin] registered source 'Neko Chat'");
}
