# Building and releasing

Everything about building neko-overlays-plugin yourself: on Linux, on Windows, and on GitHub (which also makes the
releases). To just install and use the plugin, see [README.md](README.md).

## 1. A Twitch Client ID (every build needs this)

The plugin logs in to Twitch with a Twitch app of your own. There is no client secret.

1. Register an app at https://dev.twitch.tv/console with **Client Type: Public** and the OAuth Redirect URL
   `http://localhost`. (2FA must be switched on for the Twitch account.)
2. Copy its **Client ID**.
3. Put it in a file called `.env` in the project folder (it is in `.gitignore`, so it stays out of git):

   ```
   NEKO_TWITCH_CLIENT_ID=your_client_id_here
   ```

The Client ID is built into the plugin when it is compiled. A Client ID is not a secret. If it is missing, the build
still works and prints a warning; the plugin then asks for the Client ID in the source's properties.

## 2. Build on Linux

You need: Rust (stable), a C compiler, `pkg-config`, libclang, and the OBS Studio development files (the ones that
include `libobs.pc`).

- Arch / CachyOS: `sudo pacman -S obs-studio clang pkgconf`
- Debian / Ubuntu: `libobs-dev` and `libclang-dev`

Then:

```
bash install.sh
```

This builds the plugin and installs it for the **Flatpak** version of OBS (restart OBS afterwards). For another kind of
OBS install, run `cargo build --release` and copy `target/release/libneko_overlays_plugin.so` to
`<your OBS plugins folder>/neko-overlays-plugin/bin/64bit/neko-overlays-plugin.so` (see the install table in the README).

## 3. Build on Windows, on a Windows PC

You need: Rust (the MSVC toolchain), the Visual Studio Build Tools with the "Desktop development with C++" workload,
LLVM (for bindgen), and git.

In a **Developer PowerShell for VS**, in the project folder:

```
./scripts/obs-sdk.ps1
cargo build --release
```

- `scripts/obs-sdk.ps1` runs once. It downloads the OBS headers and makes `obs.lib` (an import library for OBS's
  `obs.dll`) in `.obs-sdk/`. Add `-Version 32.2.1` to choose the OBS version it uses (see "Which OBS version" below).
- If bindgen cannot find libclang, set `LIBCLANG_PATH` to LLVM's `bin` folder, for example `C:\Program Files\LLVM\bin`.
- Copy `target\release\neko_overlays_plugin.dll` to
  `%APPDATA%\obs-studio\plugins\neko-overlays-plugin\bin\64bit\neko-overlays-plugin.dll`.
- The C runtime is linked into the DLL (`.cargo/config.toml`), so no Visual C++ redistributable is needed.

## 4. Build on Windows without a Windows PC

Let GitHub build it: see "Releases (GitHub)" below. Every push produces a Windows zip and a Linux zip.

## Which OBS version

The OBS headers (and on Windows `obs.dll`) come from one OBS release, default `32.2.1`. Use the oldest OBS you want the
plugin to run on: a plugin built against newer OBS headers than the OBS running it can be refused.

## Environment variables

| Variable                | What it does                                                                                                                |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `NEKO_TWITCH_CLIENT_ID` | The Twitch Client ID to build in. A real environment variable wins over `.env`.                                             |
| `OBS_INCLUDE_DIR`       | A folder containing `obs.h`. Without it the build looks in `.obs-sdk/obs-studio/libobs`, and on Linux then asks pkg-config. |
| `OBS_LIB_DIR`           | Windows only: the folder containing `obs.lib`. Default: `.obs-sdk/lib`.                                                     |
| `LIBCLANG_PATH`         | Windows: LLVM's `bin` folder, if bindgen cannot find libclang.                                                              |

## Releases (GitHub)

`.github/workflows/build.yml` builds the plugin for Windows (x64) and Linux (x86_64) on GitHub's machines.

One-time setup:

1. Push the project to a GitHub repository (`.env` is in `.gitignore`, so the Client ID stays out of it).
2. Settings > Secrets and variables > Actions > New repository secret: `NEKO_TWITCH_CLIENT_ID` = your Client ID.
   It is built into the plugin, the same as `.env` does locally. (A Client ID is not secret, but this keeps it out of git.)
3. Optional: a repository _variable_ `OBS_VERSION` (default `32.2.1`), see "Which OBS version" above.

For every release:

1. Raise `version` in `Cargo.toml`, run `cargo build` once so `Cargo.lock` follows (the workflow builds with `--locked`),
   and commit both.
2. `git tag v<version>` (the tag must be `v` + the Cargo version, for example `v26.1.0`) and `git push --tags`.
3. The workflow builds both zips, then publishes a release with them and `SHA256SUMS.txt`. A tag with a dash
   (`v26.2.0-beta1`) is published as a pre-release.

Pushes and pull requests build both too, and keep the zips for a while under the run's Artifacts. To build against a
different OBS version once, use the workflow's **Run workflow** button.

## If the build fails

| Message                                        | Fix                                                                                                                                                                                               |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `libobs not found` (Linux)                     | Install the OBS development files (see section 2), or set `OBS_INCLUDE_DIR`.                                                                                                                      |
| `obs.lib not found` (Windows)                  | Run `./scripts/obs-sdk.ps1`, or set `OBS_LIB_DIR`.                                                                                                                                                |
| `bindgen failed`                               | libclang is missing. Linux: install it. Windows: set `LIBCLANG_PATH`.                                                                                                                             |
| `could not compile assets/styles/infobar.scss` | The built-in Sass compiler choked on a style. Compile it yourself: `npx sass --no-source-map assets/styles/infobar.scss assets/styles/infobar.css`. The build uses that `.css` file if it exists. |
| The GitHub build fails on `--locked`           | `Cargo.lock` is out of date. Run `cargo build` locally and commit `Cargo.lock`.                                                                                                                   |

## How it works

- The plugin starts a small web server on `127.0.0.1` (port 47831, or the next free one up to 47850). It only answers
  requests addressed to this computer, and it serves only the bar's settings and numbers, never your Twitch login.
- The Neko Info Bar and Neko Chat sources each wrap an OBS browser source that shows their page from that server
  (`/bar/<id>` and `/chat/<id>`). The page and its
  styles (SCSS, compiled while building) are inside the plugin, so OBS needs its built-in Browser source.
- Twitch data comes straight from Twitch: the Helix API every 15 seconds (followers, newest follower, subscribers,
  viewers), and your own chat (read only) for subscriptions, cheers, watch streaks and the Neko Chat messages. Nothing goes
  through any other server.
- Subscribers: `poll_stats` in `src/twitch.rs` calls `fetch_subscribers`, which reads `subscriptions` in pages of 100
  (a short page ends the loop, because Twitch also sends a cursor on the last page). The count is Twitch's `total`, which
  does not include the broadcaster; the list does, so the broadcaster's own entry is skipped. The names go to the bar as
  `subscribers` in `/api/bar/<id>/state`, sorted by tier, then name, and `assets/bar.js` rotates through them
  (`SUB_RUN` names in a row). A failed request keeps the last count and names; the reason is written to the OBS log once
  (`note_subscriber_problem`), not on every poll.
- Neko Chat: `src/chat.rs` keeps the last 100 chat messages in a log (with the emote positions, badges and room id from the
  IRC tags). `CLEARMSG` (one message deleted) removes that message from the log, `CLEARCHAT` removes everything from the
  timed-out or banned user (or everything, for `/clear`). The page (`assets/chat.js`) polls `/api/chat/<id>/state` twice a
  second; the answer holds the messages that are still in the chat, so a deleted message is simply missing and the page
  takes it off the screen. The blocklist ("Hidden users") is turned into a list of lowercase names by `blocklist` in
  `src/chat_overlay.rs` and applied by the page, so editing it applies at once. The chat page itself loads the emote and badge
  pictures, and the 7TV / BTTV / FrankerFaceZ emote lists, straight from their servers (`static-cdn.jtvnw.net`,
  `badges.twitch.tv`, `cdn.7tv.app`, `7tv.io`, `betterttv.net`, `frankerfacez.com`). That is the only thing the pages load from
  the internet; a failure only means the emotes or badges are missing.
- The latest events (`latest.json`) and the login (`twitch.txt`) are kept in the plugin's config folder. See the README
  for where that is.
- Fonts: the bar page never contacts Google. The first time a style or the Font box needs a Google font, the plugin
  downloads it from `fonts.googleapis.com` / `fonts.gstatic.com` into its config folder (`fonts/`) and serves it from
  there. Until it is there the style's fallback font is shown. Your own `@font-face` CSS and font files can be dropped
  into that folder. The OBS log says what happened: lines starting with `[neko-overlays-plugin] downloaded Google font`
  or `could not get Google font`.
- Logging in: the **Connect to Twitch** button starts Twitch's device code flow and opens `http://127.0.0.1:<port>/connect`
  in the default browser (`rundll32` on Windows, `xdg-open` on Linux). That page (`assets/connect.html`) shows the code
  to copy. The code is also on the bar, and in the OBS log.
- The icons are simple built-in SVGs instead of Font Awesome. They are in `ICON_PATHS` in `assets/bar.js`.

## Checking Twitch by hand

Handy when a number on the bar looks wrong. Twitch only accepts a token together with the Client ID it was issued for, so
use the Client ID the plugin was built with, and the token from the first line of `twitch.txt` (see the README for the
folder). Never post the token anywhere.

```
# Which app, account and permissions does the token have? (no Client ID needed)
curl -H "Authorization: OAuth <userAccessToken>" https://id.twitch.tv/oauth2/validate

# The subscriber list the bar uses
curl -H "Client-Id: <clientID>" -H "Authorization: Bearer <userAccessToken>" \
  "https://api.twitch.tv/helix/subscriptions?broadcaster_id=<broadcasterID>&first=100"
```

`validate` shows `client_id`, `login`, `scopes` and `expires_in`; `channel:read:subscriptions` must be in `scopes`. The
`<broadcasterID>` is the third line of `twitch.txt`. A browser cannot run the second command, because the token has to go in
a header. A website that makes tokens with its own app (such as twitchtokengenerator.com) gives tokens for a different
Client ID, which Twitch rejects with "Client ID and OAuth token do not match".

## Files

- `src/lib.rs` the plugin entry point, `src/server.rs` local web server, `src/info_bar.rs` the Neko Info Bar OBS source,
  `src/chat_overlay.rs` the Neko Chat OBS source, `src/twitch.rs` login and Helix, `src/chat.rs` the chat connection and
  the chat log, `src/fonts.rs` Google font download and cache
- `assets/bar.js`, `assets/bar.html`, `assets/bar-extra.css`, `assets/chat.js`, `assets/chat.html`, `assets/gallery.html`,
  `assets/connect.html`, `assets/styles/*.scss` the bar and chat pages, the style gallery and the styles (`_chat.scss` is the
  chat look of every style). To change a style, edit the SCSS and rebuild.
- `build.rs` builds the OBS bindings, compiles the styles and reads the Client ID
- `install.sh` build and install on Linux (Flatpak OBS), `scripts/obs-sdk.ps1` the Windows OBS headers and `obs.lib`
- `.github/workflows/build.yml` the GitHub build and release, `.cargo/config.toml` the Windows runtime setting
