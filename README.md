<!-- Location: neko-overlays-plugin/README.md -->
<!-- File: README.md -->

# neko-overlays-plugin

An OBS plugin that adds one source, **Neko Info Bar**: a bar on your stream that rotates through your Twitch followers,
subscribers, viewers and latest events, with a subscriber train. It comes in 12 styles. Works on Windows and Linux.

## Install

1. Download the zip for your system from the **Releases** page on GitHub: `neko-overlays-plugin-<version>-windows-x64.zip`
   for Windows, `neko-overlays-plugin-<version>-linux-x86_64.zip` for Linux.
2. Close OBS.
3. Extract the zip into OBS's `plugins` folder (create the folder if it is not there yet):

   | System             | Extract into                                                               |
   | ------------------ | -------------------------------------------------------------------------- |
   | Windows            | `%APPDATA%\obs-studio\plugins\` (paste that into the Explorer address bar) |
   | Linux              | `~/.config/obs-studio/plugins/`                                            |
   | Linux, Flatpak OBS | `~/.var/app/com.obsproject.Studio/config/obs-studio/plugins/`              |

   Extract it only once, straight into that folder. Afterwards the plugin file must be exactly here:

   ```
   plugins/neko-overlays-plugin/bin/64bit/neko-overlays-plugin.dll   (Windows)
   plugins/neko-overlays-plugin/bin/64bit/neko-overlays-plugin.so    (Linux)
   ```

   On Windows, "Extract All" suggests a destination that ends in the zip's own name. Delete that last part, so the
   destination ends in `plugins\`. If you already ended up with a folder inside a folder, move the inner
   `neko-overlays-plugin` folder up into `plugins`.

4. Start OBS and add the source: Sources > **+** > **Neko Info Bar**.
5. In the source's properties, click **Connect to Twitch**. Your browser opens a page with a code. Click **Copy code**,
   then **Open Twitch**, paste the code and approve. (If the page does not open, go to `http://127.0.0.1:47831/connect`.)

Updating: close OBS and extract the new zip the same way. If the bar asks you to connect again, click **Connect to Twitch**.

## Use it

- Pick a style, colors, font and what the bar shows in the source's properties. The bar is transparent; the style draws
  its own box.
- The subscriber slot rotates through everyone who is subscribed, with their tier (for example "TheFukinMoneyBadger
  Tier 2"), a few names at a time. You are never listed yourself. A brand-new subscriber is shown right away. It uses the
  **Latest subscriber** switch in the properties.
- To see every style with made-up numbers, open `http://127.0.0.1:47831/` in a browser while OBS is running.
- Fonts: the styles use Google fonts. The plugin downloads each one once the first time it is needed, and after that it
  works offline. To turn this off, untick **Download Google Fonts** in the properties.

## Where the data comes from

| Bar item                   | Source                                                                                                |
| -------------------------- | ----------------------------------------------------------------------------------------------------- |
| Followers, latest follower | Helix `channels/followers`, polled. A new newest follower counts as a new follower (up to 15 s late). |
| Subscribers (number)       | Helix `subscriptions` (affiliates and partners only), polled                                          |
| Subscriber names and tiers | The same Helix `subscriptions` list, without you. Shown in rotation.                                  |
| Viewers                    | Helix `streams` (0 while offline)                                                                     |
| Latest subscriber (months) | Chat: sub, resub and gifted sub notices. Shown the moment it happens.                                 |
| Latest cheer (bits)        | Chat: messages with bits                                                                              |
| Latest watch streak        | Chat: watch streak milestone notices                                                                  |

Followers do not appear in chat, which is why they are polled. Twitch's EventSub would make them instant; that is a
possible later step.

Notes on the subscriber list: Twitch lists the channel owner too, but leaves them out of the count, so the plugin skips
that entry. It does not read "who subscribed last" from the list, because Twitch gives no subscription dates and no
promised order; the list is sorted by tier, then by name. A resub that the viewer does not share is not announced by
Twitch in chat, so it will not show as a new event.

## Where the plugin keeps its files

The plugin's config folder (OBS decides the exact place):

| System             | Folder                                                                                   |
| ------------------ | ---------------------------------------------------------------------------------------- |
| Windows            | `%APPDATA%\obs-studio\plugin_config\neko-overlays-plugin\`                               |
| Linux              | `~/.config/obs-studio/plugin_config/neko-overlays-plugin/`                               |
| Linux, Flatpak OBS | `~/.var/app/com.obsproject.Studio/config/obs-studio/plugin_config/neko-overlays-plugin/` |

- `twitch.txt` is your Twitch login. Keep it private, and never post it. Clicking **Disconnect from Twitch** deletes it.
- `latest.json` holds the latest follower, subscriber, cheer and watch streak, so they survive restarting OBS. You can
  edit it while OBS is closed, for example to put in a streak from before you installed the plugin:
  `{"streaker":{"name":"SomeViewer","amount":12,"seq":1}}` (keep the other entries if the file has them).
- `fonts/` holds the downloaded Google fonts.

## If something is missing

The OBS log (Help > Log Files > View Current Log) has lines starting with `[neko-overlays-plugin]`.

| What you see                            | What to do                                                                                                                                                                                                                                                                                          |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Subscribers shows 0, or no names rotate | Search the log for `subscriber count unavailable`. If it mentions a missing scope, click **Disconnect from Twitch**, then **Connect to Twitch**, and approve all permissions. Subscriber data needs a Twitch affiliate or partner channel. A failed request keeps the last known numbers and names. |
| "No Subscribers" in the latest slot     | No sub has happened while OBS was running yet, and the list was empty or failed. Wait for the next sub, or put one in `latest.json`.                                                                                                                                                                |
| The bar says to connect again           | Click **Connect to Twitch** in the source's properties.                                                                                                                                                                                                                                             |

## Build it yourself

See [BUILD.md](BUILD.md).
