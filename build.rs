// Location: neko-overlays-plugin/build.rs
// File: build.rs

use std::{env, path::PathBuf};

/// Bakes the Twitch Client ID into the plugin. Order: real environment variable first,
/// then `.env` next to Cargo.toml. twitch.rs picks it up through `option_env!`.
/// A Client ID is public (it is visible in every Twitch login), but `.env` stays out of git.
fn load_client_id() {
    const KEY: &str = "NEKO_TWITCH_CLIENT_ID";
    const PLACEHOLDER: &str = "paste_your_client_id_here";

    println!("cargo:rerun-if-env-changed={KEY}");
    println!("cargo:rerun-if-changed=.env");

    let usable = |v: &str| !v.is_empty() && v != PLACEHOLDER;

    if let Ok(value) = env::var(KEY) {
        let value = value.trim().to_string();
        if usable(&value) {
            println!("cargo:rustc-env={KEY}={value}");
            return;
        }
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    if let Ok(text) = std::fs::read_to_string(manifest_dir.join(".env")) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((name, value)) = line.split_once('=') else { continue };
            if name.trim() != KEY {
                continue;
            }
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            if usable(value) {
                println!("cargo:rustc-env={KEY}={value}");
                return;
            }
        }
    }

    println!("cargo:warning={KEY} not found in the environment or .env: this build has no built-in Twitch Client ID");
}

/// Compiles the info bar styles into $OUT_DIR/infobar.css, which server.rs embeds in the plugin.
/// If assets/styles/infobar.css exists (made with Dart Sass: `npx sass --no-source-map assets/styles/infobar.scss assets/styles/infobar.css`)
/// it is used as it is instead. That is the way out if the built-in Sass compiler ever chokes on a style.
fn compile_styles() {
    println!("cargo:rerun-if-changed=assets/styles");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("infobar.css");

    let precompiled = manifest_dir.join("assets/styles/infobar.css");
    if precompiled.exists() {
        std::fs::copy(&precompiled, &out).expect("could not copy assets/styles/infobar.css");
        return;
    }

    let entry = manifest_dir.join("assets/styles/infobar.scss");
    let css = grass::from_path(entry.to_str().unwrap(), &grass::Options::default())
        .unwrap_or_else(|e| panic!("could not compile assets/styles/infobar.scss: {e}"));
    std::fs::write(&out, css).expect("could not write infobar.css");
}

fn main() {
    load_client_id();
    compile_styles();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // The target, not the machine running the build (they differ when cross-compiling).
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:rerun-if-env-changed=OBS_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=OBS_LIB_DIR");

    // Where the OBS headers come from, in this order:
    //   1. OBS_INCLUDE_DIR, a folder containing obs.h
    //   2. .obs-sdk/obs-studio/libobs in this folder (scripts/obs-sdk.ps1 and the GitHub workflow make it)
    //   3. Linux only: pkg-config (Arch/CachyOS: `pacman -S obs-studio` ships libobs.pc)
    let sdk = manifest_dir.join(".obs-sdk");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = env::var("OBS_INCLUDE_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(sdk.join("obs-studio/libobs"));
    let have_sdk_headers = candidates.iter().any(|dir| dir.join("obs.h").exists());

    if target_os == "windows" {
        // Windows needs an import library for obs.dll (scripts/obs-sdk.ps1 makes it from the OBS install).
        let lib_dir = env::var("OBS_LIB_DIR").map(PathBuf::from).unwrap_or_else(|_| sdk.join("lib"));
        assert!(
            lib_dir.join("obs.lib").exists(),
            "obs.lib not found in {}. Run scripts/obs-sdk.ps1, or set OBS_LIB_DIR to the folder containing obs.lib",
            lib_dir.display()
        );
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=dylib=obs");
    } else if !have_sdk_headers {
        // Finds libobs via pkg-config, emits the link flags, and gives us the header include paths.
        let lib = pkg_config::Config::new()
            .probe("libobs")
            .expect("libobs not found: install the OBS Studio dev files (Arch: obs-studio), or set OBS_INCLUDE_DIR");
        candidates.extend(lib.include_paths.clone());
        // Arch's libobs.pc doesn't emit -I for /usr/include/obs
        candidates.push(PathBuf::from("/usr/include/obs"));
        candidates.push(PathBuf::from("/usr/local/include/obs"));
    }
    // else: Linux with the SDK headers. Nothing to link: OBS provides libobs when it loads the plugin.

    let mut include_dirs: Vec<PathBuf> = Vec::new();
    for dir in candidates {
        if dir.join("obs.h").exists() && !include_dirs.contains(&dir) {
            include_dirs.push(dir);
        }
    }
    assert!(
        !include_dirs.is_empty(),
        "obs.h not found: install obs-studio, run scripts/obs-sdk.ps1, or set OBS_INCLUDE_DIR to the folder containing obs.h"
    );

    // OBS's source tree (what the GitHub workflow and scripts/obs-sdk.ps1 download) has no obsconfig.h: CMake makes it
    // when OBS is built, and obs-config.h includes it. An installed OBS has one. Without it, write a stand-in that
    // only holds the few values the headers mention, so bindgen can parse them.
    if !include_dirs.iter().any(|d| d.join("obsconfig.h").exists()) {
        let stub_dir = PathBuf::from(env::var("OUT_DIR").unwrap()).join("obsconfig-stub");
        std::fs::create_dir_all(&stub_dir).expect("could not create the obsconfig stub folder");
        std::fs::write(
            stub_dir.join("obsconfig.h"),
            "#pragma once\n\
             #define OBS_DATA_PATH \"data\"\n\
             #define OBS_PLUGIN_PATH \"plugins\"\n\
             #define OBS_PLUGIN_DESTINATION \"plugins\"\n\
             #define OBS_RELEASE_CANDIDATE 0\n\
             #define OBS_BETA 0\n",
        )
        .expect("could not write the obsconfig.h stand-in");
        include_dirs.push(stub_dir);
    }

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_function("obs_.*|gs_.*|blog|bfree")
        .allowlist_type("obs_.*|gs_.*")
        .allowlist_var("LIBOBS_API_.*|OBS_.*|LOG_.*")
        .derive_default(true)
        // bindgen mis-sizes vec2/vec3/vec4, obs_transform_info, obs_audio_data and
        // obs_source_frame (anonymous unions / SIMD types), which fails its own layout
        // self-tests. We don't use those types yet, so skip the tests. Do NOT pass those
        // types by value to OBS until their layout is verified.
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    // With the MSVC target, libobs's util_uint64.h calls _udiv128, which clang's own intrin.h doesn't declare. Newer
    // clang makes an undeclared function an error. It sits in an inline function bindgen never uses, so allow it.
    builder = builder.clang_arg("-Wno-implicit-function-declaration");

    for dir in &include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }
    // libobs.pc defines this so the headers pick up obsconfig.h (an installed OBS has one, the source tree doesn't)
    if include_dirs.iter().any(|d| d.join("obsconfig.h").exists()) {
        builder = builder.clang_arg("-DHAVE_OBSCONFIG_H");
    }

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    builder
        .generate()
        .expect("bindgen failed (is libclang installed? on Windows set LIBCLANG_PATH to LLVM's bin folder)")
        .write_to_file(out.join("bindings.rs"))
        .expect("could not write bindings.rs");
} 