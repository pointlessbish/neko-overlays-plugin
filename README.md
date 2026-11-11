<!-- Location: neko-overlays-plugin/README.md -->
<!-- File: README.md -->

# neko-overlays-plugin

An OBS plugin with one source, **Neko Info Bar**: the info bar from the neko overlays, with all 19 styles, the
rotating stats and latest events, and the subscriber/follower train. Everything runs on your own computer.
The bar page loads nothing from the internet, and nothing goes through Cloudflare or pointlessbish.wtf. The only
other thing the plugin fetches is Google fonts, once each (see Fonts).

## How it works

- The plugin starts a small web server on `127.0.0.1` (port 47831, or the next free one up to 47850). It only answers
  requests addressed to this computer.
- The Neko Info Bar source wraps an OBS browser source that shows the bar page from that server. The styles (the
  SCSS from neko-overlays, compiled while building) and the page script are inside the plugin.
- Twitch data comes straight from Twitch: the Helix API every 15 seconds (followers, newest follower, subscribers,
  viewers), and your own chat (read only) for subscriptions, cheers and watch streaks.
- Needs the browser source that ships with OBS (the Windows installer, the Flatpak version and most distributions have it).

## Install a release (Windows or Linux)

1. Download the zip for your system from the Releases page on GitHub.
2. Extract it into OBS's plugins folder, so that `neko-overlays-plugin/bin/64bit/` ends up inside it:
   - Windows: `%APPDATA%\obs-studio\plugins\` (paste that into the Explorer address bar)
   - Linux: `~/.config/obs-studio/plugins/`, or for the Flatpak version `~/.var/app/com.obsproject.Studio/config/obs-studio/plugins/`
3. Restart OBS, then Sources > + > **Neko Info Bar**. Click **Connect to Twitch**, open the address shown on the bar,
   enter the code.

If you connected with an older version, click **Connect to Twitch** once more: chat needs a new permission (`chat:read`).

## Build it yourself

Everything starts with a Twitch app: register one at https://dev.twitch.tv/console with **Client Type: Public** and the
OAuth Redirect URL `http://localhost` (2FA must be on for the Twitch account). Copy its Client ID. There is no client
secret. Put the Client ID in `.env`: `NEKO_TWITCH_CLIENT_ID=...`

- **Linux:** `bash install.sh` (builds, and installs for the Flatpak version of OBS), then restart OBS.
- **Windows, without a Windows PC:** let GitHub build it, see Releases below. Every push produces both zips.
- **Windows, on a Windows PC:** install Rust (the MSVC one), the Visual Studio Build Tools (C++), LLVM and git. In a
  "Developer PowerShell for VS": `./scripts/obs-sdk.ps1` (gets the OBS headers and makes `obs.lib`, once), then
  `cargo build --release`, then copy `target\release\neko_overlays_plugin.dll` to
  `%APPDATA%\obs-studio\plugins\neko-overlays-plugin\bin\64bit\neko-overlays-plugin.dll`.

## Releases (GitHub)

`.github/workflows/build.yml` builds the plugin for Windows (x64) and Linux (x86_64) on GitHub's machines.

One-time setup:

1. Push the project to a GitHub repository (`.env` is in `.gitignore`, so the Client ID stays out of it).
2. Settings > Secrets and variables > Actions > New repository secret: `NEKO_TWITCH_CLIENT_ID` = your Client ID.
   It is built into the plugin, the same as `.env` does locally. (A Client ID is not secret, but this keeps it out of git.)
3. Optional: a repository _variable_ `OBS_VERSION` (default `32.2.1`). Use the oldest OBS you want to support: a plugin
   built against newer OBS headers than the OBS running it can be refused.

For every release:

1. Raise `version` in `Cargo.toml`, run `cargo build` once so `Cargo.lock` follows (the workflow builds with `--locked`),
   and commit both.
2. `git tag v0.3.0` (the tag must be `v` + the Cargo version) and `git push --tags`.
3. The workflow builds both zips, then publishes a release with them and `SHA256SUMS.txt`. A tag with a dash
   (`v0.3.0-beta1`) is published as a pre-release.

Pushes and pull requests build both too, and keep the zips for a while under the run's Artifacts.

## See all the styles

Open `http://127.0.0.1:47831/` in any browser while OBS is running: every style with made-up numbers.
In OBS you pick the style in the source's properties.

## Fonts

The styles use Google fonts (Inter, Audiowide, Oswald, Orbitron, ...), and you can type any Google font name into the
source's **Font** box. You do not need to install any of them.

- The first time a style or the Font box needs a font, the plugin downloads it from Google Fonts and keeps it in its
  config folder (`fonts/`). From then on it is served from your computer, so it keeps working offline.
- The bar page itself never contacts Google. Only the plugin does, and only for `fonts.googleapis.com` and
  `fonts.gstatic.com`.
- Until the download is done (a few seconds, once) the style's fallback font is shown; the bar then rebuilds itself
  with the real one.
- To turn this off, untick **Download Google Fonts** in the source's properties. Fonts already downloaded keep
  working, and you can also drop your own `@font-face` CSS and font files into the `fonts/` folder.
- A name that is not a Google font (or no connection) just keeps the fallback fonts. The OBS log says what happened:
  lines starting with `[neko-overlays-plugin] downloaded Google font` or `could not get Google font`.

## Properties

Style, own colors, font, font size, Google font download on/off, seconds per item, which numbers and latest events to show, the train (type and
seconds), and the size of the source. The bar is transparent; the style draws its own box.

## Where the data comes from

| Bar item                   | Source                                                                                                |
| -------------------------- | ----------------------------------------------------------------------------------------------------- |
| Followers, latest follower | Helix `channels/followers`, polled. A new newest follower counts as a new follower (up to 15 s late). |
| Subscribers (number)       | Helix `subscriptions` (affiliates and partners only)                                                  |
| Viewers                    | Helix `streams` (0 while offline)                                                                     |
| Latest subscriber (months) | Chat: sub, resub and gifted sub notices                                                               |
| Latest cheer (bits)        | Chat: messages with bits                                                                              |
| Latest watch streak        | Chat: watch streak milestone notices                                                                  |

The latest events are kept in `latest.json` in the plugin's config folder, so they survive restarting OBS.
Followers do not appear in chat, which is why they are polled. Twitch's EventSub would make them instant; that is a
possible later step.

## Files

- `src/server.rs` local web server, `src/info_bar.rs` the OBS source, `src/twitch.rs` login and Helix, `src/chat.rs` chat,
  `src/fonts.rs` Google font download and cache
- `assets/bar.js`, `assets/bar.html`, `assets/bar-extra.css`, `assets/styles/*.scss` the bar page and its styles
- To change a style, edit the SCSS and rebuild. If the built-in Sass compiler ever fails on a style, compile it yourself
  (`npx sass --no-source-map assets/styles/infobar.scss assets/styles/infobar.css`); the build uses that file if it exists.

## Notes

- Icons are simple built-in SVGs instead of Font Awesome. They are in `ICON_PATHS` in `assets/bar.js`.
- The local server serves only the bar's own settings and numbers, never your Twitch login.
- Windows (x64) and Linux (x86_64). There is no macOS build.
