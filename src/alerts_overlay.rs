use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use serde_json::{json, Map, Value};

use crate::alerts;
use crate::ffi::*;
use crate::log_info;
use crate::server::{self, THEMES};
use crate::twitch;

const DEFAULT_BACKGROUND: i64 = 0xFF14_0026;
const DEFAULT_ACCENT: i64 = 0xFFC0_A6F1;
const DEFAULT_TEXT: i64 = 0xFFFC_FFFD;

// The alert types that can have their own picture, video and sound, with the name shown in the properties.
const MEDIA_TYPES: [(&str, &str); 5] = [
    ("follow", "Follower"),
    ("sub", "Subscription or resub"),
    ("gift", "Gifted subs"),
    ("cheer", "Cheer"),
    ("raid", "Raid"),
];

const VISUAL_FILTER: &CStr = c"Pictures and videos (*.png *.jpg *.jpeg *.gif *.webp *.apng *.avif *.svg *.mp4 *.m4v *.webm *.ogv);;All files (*.*)";
const SOUND_FILTER: &CStr = c"Sounds (*.mp3 *.ogg *.oga *.wav *.m4a *.aac *.flac);;All files (*.*)";

const BROWSER_CSS: &CStr =
    c"body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }";

struct AlertsOverlay {
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
    format!(
        "#{:02x}{:02x}{:02x}",
        color & 0xff,
        (color >> 8) & 0xff,
        (color >> 16) & 0xff
    )
}

fn clean_font(name: &str) -> Option<String> {
    let name = name.trim();
    let plain = !name.is_empty()
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
    plain.then(|| name.to_string())
}

/// The file picked for one alert type and slot, if it is the right kind of file and exists.
/// `allowed` lists the kinds accepted: "image" and "video" for the visual, "audio" for the sound.
unsafe fn media_path(
    settings: *mut obs_data_t,
    kind: &str,
    slot: &str,
    allowed: &[&str],
) -> Option<(String, &'static str)> {
    let key = CString::new(format!("{kind}_{slot}")).ok()?;
    let path = get_str(settings, &key);
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let (class, _) = alerts::media_kind(path)?;
    if !allowed.contains(&class) || !std::path::Path::new(path).is_file() {
        return None;
    }
    Some((path.to_string(), class))
}

unsafe fn page_settings(settings: *mut obs_data_t) -> Value {
    let theme = get_str(settings, c"theme");
    let theme = if THEMES.iter().any(|(id, _)| *id == theme) {
        theme
    } else {
        String::from("classic")
    };

    let mut colors = Map::new();
    if get_bool(settings, c"custom_colors") {
        colors.insert(
            "accent".into(),
            json!(hex(obs_data_get_int(settings, c"accent_color".as_ptr()))),
        );
        colors.insert(
            "background".into(),
            json!(hex(obs_data_get_int(
                settings,
                c"background_color".as_ptr()
            ))),
        );
        colors.insert(
            "text".into(),
            json!(hex(obs_data_get_int(settings, c"text_color".as_ptr()))),
        );
        colors.insert(
            "textAccent".into(),
            json!(hex(obs_data_get_int(
                settings,
                c"text_accent_color".as_ptr()
            ))),
        );
    }

    let font_size = get_int(settings, c"font_size", 0, 96);
    let font_size = if font_size > 0 {
        json!(font_size)
    } else {
        Value::Null
    };

    // "_files" holds the paths and stays inside the plugin (server.rs removes it before the page sees the settings);
    // "media" tells the page which slots have a file and what kind.
    let mut files = Map::new();
    let mut media = Map::new();
    for (kind, _) in MEDIA_TYPES {
        let mut kind_files = Map::new();
        let mut kind_media = Map::new();
        if let Some((path, class)) = media_path(settings, kind, "visual", &["image", "video"]) {
            kind_files.insert("visual".into(), json!(path));
            kind_media.insert("visual".into(), json!(class));
        }
        if let Some((path, _)) = media_path(settings, kind, "sound", &["audio"]) {
            kind_files.insert("sound".into(), json!(path));
            kind_media.insert("sound".into(), json!(true));
        }
        if !kind_files.is_empty() {
            files.insert(kind.into(), Value::Object(kind_files));
            media.insert(kind.into(), Value::Object(kind_media));
        }
    }

    // Seconds per alert type; 0 (or missing) means the general time.
    let mut durations = Map::new();
    for (kind, _) in MEDIA_TYPES {
        let key = CString::new(format!("{kind}_duration")).unwrap_or_default();
        durations.insert(kind.into(), json!(get_int(settings, &key, 0, 60)));
    }

    json!({
        "durations": durations,
        "_files": files,
        "media": media,
        "soundVolume": get_int(settings, c"sound_volume", 0, 100),
        "showText": get_bool(settings, c"show_text"),
        "theme": theme,
        "colors": colors,
        "font": clean_font(&get_str(settings, c"font")),
        "fontSize": font_size,
        "googleFonts": get_bool(settings, c"google_fonts"),
        "durationSeconds": get_int(settings, c"duration_seconds", 2, 60),
        "types": {
            "follow": get_bool(settings, c"show_follows"),
            "sub": get_bool(settings, c"show_subs"),
            "gift": get_bool(settings, c"show_gifts"),
            "cheer": get_bool(settings, c"show_cheers"),
            "raid": get_bool(settings, c"show_raids"),
        },
        "minBits": get_int(settings, c"min_bits", 1, 1_000_000),
        "minRaiders": get_int(settings, c"min_raiders", 1, 1_000_000),
        "showMessage": get_bool(settings, c"show_message"),
    })
}

unsafe fn apply_browser(overlay: &mut AlertsOverlay) {
    let port = server::port();
    if port == 0 {
        return;
    }
    let url = server::alerts_url(overlay.id);
    let wanted = (url.clone(), overlay.width, overlay.height);
    if overlay.applied.as_ref() == Some(&wanted) {
        return;
    }
    let url = CString::new(url).unwrap_or_default();

    let settings = obs_data_create();
    obs_data_set_string(settings, c"url".as_ptr(), url.as_ptr());
    obs_data_set_bool(settings, c"is_local_file".as_ptr(), false);
    obs_data_set_int(settings, c"width".as_ptr(), overlay.width as i64);
    obs_data_set_int(settings, c"height".as_ptr(), overlay.height as i64);
    obs_data_set_bool(settings, c"shutdown".as_ptr(), false);
    obs_data_set_bool(settings, c"restart_when_active".as_ptr(), false);
    obs_data_set_bool(settings, c"reroute_audio".as_ptr(), false);
    obs_data_set_bool(settings, c"fps_custom".as_ptr(), false);
    obs_data_set_string(settings, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    obs_source_update(overlay.browser, settings);
    obs_data_release(settings);

    overlay.applied = Some(wanted);
}

unsafe extern "C" fn alerts_get_name(_type_data: *mut c_void) -> *const c_char {
    c"Neko Alerts".as_ptr()
}

unsafe extern "C" fn alerts_get_defaults(settings: *mut obs_data_t) {
    obs_data_set_default_string(settings, c"theme".as_ptr(), c"classic".as_ptr());
    obs_data_set_default_bool(settings, c"custom_colors".as_ptr(), false);
    obs_data_set_default_int(settings, c"accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_int(settings, c"background_color".as_ptr(), DEFAULT_BACKGROUND);
    obs_data_set_default_int(settings, c"text_color".as_ptr(), DEFAULT_TEXT);
    obs_data_set_default_int(settings, c"text_accent_color".as_ptr(), DEFAULT_ACCENT);
    obs_data_set_default_string(settings, c"font".as_ptr(), c"".as_ptr());
    obs_data_set_default_int(settings, c"font_size".as_ptr(), 0);
    obs_data_set_default_bool(settings, c"google_fonts".as_ptr(), true);

    obs_data_set_default_int(settings, c"duration_seconds".as_ptr(), 8);
    obs_data_set_default_bool(settings, c"show_follows".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_subs".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_gifts".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_cheers".as_ptr(), true);
    obs_data_set_default_bool(settings, c"show_raids".as_ptr(), true);
    obs_data_set_default_int(settings, c"min_bits".as_ptr(), 1);
    obs_data_set_default_int(settings, c"min_raiders".as_ptr(), 1);
    obs_data_set_default_bool(settings, c"show_message".as_ptr(), true);
    for (kind, _) in MEDIA_TYPES {
        let key = CString::new(format!("{kind}_duration")).unwrap_or_default();
        obs_data_set_default_int(settings, key.as_ptr(), 0);
    }
    obs_data_set_default_int(settings, c"sound_volume".as_ptr(), 70);
    obs_data_set_default_bool(settings, c"show_text".as_ptr(), true);

    obs_data_set_default_int(settings, c"width".as_ptr(), 700);
    obs_data_set_default_int(settings, c"height".as_ptr(), 400);
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

// One callback per button: OBS gives a button callback no way to tell which button was clicked.
unsafe extern "C" fn test_follow(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::FOLLOW);
    false
}

unsafe extern "C" fn test_sub(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::SUB);
    false
}

unsafe extern "C" fn test_resub(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::RESUB);
    false
}

unsafe extern "C" fn test_gift(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::GIFT);
    false
}

unsafe extern "C" fn test_mystery_gift(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::MYSTERY_GIFT);
    false
}

unsafe extern "C" fn test_cheer(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::CHEER);
    false
}

unsafe extern "C" fn test_raid(
    _p: *mut obs_properties_t,
    _b: *mut obs_property_t,
    _d: *mut c_void,
) -> bool {
    alerts::push_test(alerts::RAID);
    false
}

unsafe extern "C" fn alerts_get_properties(_data: *mut c_void) -> *mut obs_properties_t {
    let props = obs_properties_create();

    obs_properties_add_button(
        props,
        c"connect".as_ptr(),
        c"Connect to Twitch".as_ptr(),
        Some(on_connect),
    );
    obs_properties_add_button(
        props,
        c"disconnect".as_ptr(),
        c"Disconnect from Twitch".as_ptr(),
        Some(on_disconnect),
    );

    obs_properties_add_button(
        props,
        c"test_follow".as_ptr(),
        c"Test: new follower".as_ptr(),
        Some(test_follow),
    );
    obs_properties_add_button(
        props,
        c"test_sub".as_ptr(),
        c"Test: new subscriber".as_ptr(),
        Some(test_sub),
    );
    obs_properties_add_button(
        props,
        c"test_resub".as_ptr(),
        c"Test: resub with message".as_ptr(),
        Some(test_resub),
    );
    obs_properties_add_button(
        props,
        c"test_gift".as_ptr(),
        c"Test: gifted sub".as_ptr(),
        Some(test_gift),
    );
    obs_properties_add_button(
        props,
        c"test_mystery_gift".as_ptr(),
        c"Test: 5 gifted subs".as_ptr(),
        Some(test_mystery_gift),
    );
    obs_properties_add_button(
        props,
        c"test_cheer".as_ptr(),
        c"Test: cheer".as_ptr(),
        Some(test_cheer),
    );
    obs_properties_add_button(
        props,
        c"test_raid".as_ptr(),
        c"Test: raid".as_ptr(),
        Some(test_raid),
    );

    let theme = obs_properties_add_list(props, c"theme".as_ptr(), c"Style".as_ptr(), 2, 3);
    for (id, label) in THEMES {
        let (id, label) = (
            CString::new(id).unwrap_or_default(),
            CString::new(label).unwrap_or_default(),
        );
        obs_property_list_add_string(theme, label.as_ptr(), id.as_ptr());
    }
    obs_properties_add_bool(
        props,
        c"custom_colors".as_ptr(),
        c"Use my own colors (otherwise the style's own)".as_ptr(),
    );
    obs_properties_add_color(
        props,
        c"accent_color".as_ptr(),
        c"Accent (borders, glow)".as_ptr(),
    );
    obs_properties_add_color(props, c"background_color".as_ptr(), c"Background".as_ptr());
    obs_properties_add_color(props, c"text_color".as_ptr(), c"Text".as_ptr());
    obs_properties_add_color(
        props,
        c"text_accent_color".as_ptr(),
        c"Names and highlights".as_ptr(),
    );
    obs_properties_add_text(
        props,
        c"font".as_ptr(),
        c"Font (empty = the style's own)".as_ptr(),
        0,
    );
    obs_properties_add_int(
        props,
        c"font_size".as_ptr(),
        c"Font size in px (0 = the style's own)".as_ptr(),
        0,
        96,
        1,
    );
    obs_properties_add_bool(
        props,
        c"google_fonts".as_ptr(),
        c"Download Google Fonts when a style or the Font box needs one (once)".as_ptr(),
    );

    obs_properties_add_int(
        props,
        c"duration_seconds".as_ptr(),
        c"Show each alert for this many seconds".as_ptr(),
        2,
        60,
        1,
    );
    obs_properties_add_bool(
        props,
        c"show_follows".as_ptr(),
        c"Alert on new followers".as_ptr(),
    );
    obs_properties_add_bool(
        props,
        c"show_subs".as_ptr(),
        c"Alert on subscriptions and resubs".as_ptr(),
    );
    obs_properties_add_bool(
        props,
        c"show_gifts".as_ptr(),
        c"Alert on gifted subs".as_ptr(),
    );
    obs_properties_add_bool(props, c"show_cheers".as_ptr(), c"Alert on cheers".as_ptr());
    obs_properties_add_int(
        props,
        c"min_bits".as_ptr(),
        c"Smallest cheer that gets an alert (bits)".as_ptr(),
        1,
        1_000_000,
        1,
    );
    obs_properties_add_bool(props, c"show_raids".as_ptr(), c"Alert on raids".as_ptr());
    obs_properties_add_int(
        props,
        c"min_raiders".as_ptr(),
        c"Smallest raid that gets an alert (viewers)".as_ptr(),
        1,
        1_000_000,
        1,
    );
    obs_properties_add_bool(
        props,
        c"show_message".as_ptr(),
        c"Show the viewer's message (resubs and cheers)".as_ptr(),
    );

    for (kind, label) in MEDIA_TYPES {
        let visual = CString::new(format!("{kind}_visual")).unwrap_or_default();
        let visual_label =
            CString::new(format!("{label}: picture, GIF or video")).unwrap_or_default();
        // 0 = OBS_PATH_FILE (pick one existing file)
        obs_properties_add_path(
            props,
            visual.as_ptr(),
            visual_label.as_ptr(),
            0,
            VISUAL_FILTER.as_ptr(),
            ptr::null(),
        );
        let sound = CString::new(format!("{kind}_sound")).unwrap_or_default();
        let sound_label = CString::new(format!("{label}: sound")).unwrap_or_default();
        obs_properties_add_path(
            props,
            sound.as_ptr(),
            sound_label.as_ptr(),
            0,
            SOUND_FILTER.as_ptr(),
            ptr::null(),
        );
        let duration = CString::new(format!("{kind}_duration")).unwrap_or_default();
        let duration_label = CString::new(format!(
            "{label}: show for this many seconds (0 = the time above)"
        ))
        .unwrap_or_default();
        obs_properties_add_int(props, duration.as_ptr(), duration_label.as_ptr(), 0, 60, 1);
    }
    obs_properties_add_int(
        props,
        c"sound_volume".as_ptr(),
        c"Sound and video volume (0 to 100)".as_ptr(),
        0,
        100,
        1,
    );
    obs_properties_add_bool(
        props,
        c"show_text".as_ptr(),
        c"Also show the text card (off = only the picture or video, when one is set)".as_ptr(),
    );

    obs_properties_add_int(props, c"width".as_ptr(), c"Width".as_ptr(), 100, 4000, 1);
    obs_properties_add_int(props, c"height".as_ptr(), c"Height".as_ptr(), 20, 4000, 1);
    props
}

unsafe extern "C" fn alerts_create(
    settings: *mut obs_data_t,
    _source: *mut obs_source_t,
) -> *mut c_void {
    let start = obs_data_create();
    obs_data_set_string(start, c"url".as_ptr(), c"about:blank".as_ptr());
    obs_data_set_int(start, c"width".as_ptr(), 700);
    obs_data_set_int(start, c"height".as_ptr(), 400);
    obs_data_set_string(start, c"css".as_ptr(), BROWSER_CSS.as_ptr());
    let browser = obs_source_create_private(
        c"browser_source".as_ptr(),
        c"neko-alerts-browser".as_ptr(),
        start,
    );
    obs_data_release(start);

    if browser.is_null() {
        log_info(c"[neko-overlays-plugin] this OBS has no browser source, so the alerts overlay cannot be shown (the Flatpak version of OBS has it)");
        return ptr::null_mut();
    }

    let overlay = Box::into_raw(Box::new(AlertsOverlay {
        browser,
        id: server::register_bar(),
        width: 700,
        height: 400,
        applied: None,
    }));
    alerts_update(overlay as *mut c_void, settings);
    overlay as *mut c_void
}

unsafe extern "C" fn alerts_destroy(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let overlay = Box::from_raw(data as *mut AlertsOverlay);
    server::remove_bar(overlay.id);
    obs_source_release(overlay.browser);
}

unsafe extern "C" fn alerts_update(data: *mut c_void, settings: *mut obs_data_t) {
    let overlay = &mut *(data as *mut AlertsOverlay);

    overlay.width = get_int(settings, c"width", 100, 4000) as u32;
    overlay.height = get_int(settings, c"height", 20, 4000) as u32;

    server::update_bar(overlay.id, page_settings(settings));
    apply_browser(overlay);
}

// The browser source must be told when it is shown, or it does not paint.
unsafe extern "C" fn alerts_show(data: *mut c_void) {
    obs_source_inc_showing((*(data as *mut AlertsOverlay)).browser);
}

unsafe extern "C" fn alerts_hide(data: *mut c_void) {
    obs_source_dec_showing((*(data as *mut AlertsOverlay)).browser);
}

unsafe extern "C" fn alerts_activate(data: *mut c_void) {
    obs_source_inc_active((*(data as *mut AlertsOverlay)).browser);
}

unsafe extern "C" fn alerts_deactivate(data: *mut c_void) {
    obs_source_dec_active((*(data as *mut AlertsOverlay)).browser);
}

unsafe extern "C" fn alerts_video_render(data: *mut c_void, _effect: *mut gs_effect_t) {
    obs_source_video_render((*(data as *mut AlertsOverlay)).browser);
}

unsafe extern "C" fn alerts_get_width(data: *mut c_void) -> u32 {
    (*(data as *mut AlertsOverlay)).width
}

unsafe extern "C" fn alerts_get_height(data: *mut c_void) -> u32 {
    (*(data as *mut AlertsOverlay)).height
}

pub fn register() {
    let mut info: obs_source_info = unsafe { std::mem::zeroed() };
    info.id = c"neko_alerts".as_ptr();
    info.type_ = 0;
    // CUSTOM_DRAW: the browser source draws itself; without the flag every frame fails with "gs_effect_loop: An effect is already active".
    info.output_flags = (OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW) as u32;
    info.get_name = Some(alerts_get_name);
    info.create = Some(alerts_create);
    info.destroy = Some(alerts_destroy);
    info.update = Some(alerts_update);
    info.get_defaults = Some(alerts_get_defaults);
    info.get_properties = Some(alerts_get_properties);
    info.show = Some(alerts_show);
    info.hide = Some(alerts_hide);
    info.activate = Some(alerts_activate);
    info.deactivate = Some(alerts_deactivate);
    info.video_render = Some(alerts_video_render);
    info.get_width = Some(alerts_get_width);
    info.get_height = Some(alerts_get_height);

    unsafe {
        obs_register_source_s(&info, std::mem::size_of::<obs_source_info>());
    }
    log_info(c"[neko-overlays-plugin] registered source 'Neko Alerts'");
}
