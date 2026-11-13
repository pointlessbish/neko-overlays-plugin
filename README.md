# neko-overlays-plugin

An OBS plugin that adds two sources. **Neko Info Bar** is a bar on your stream that rotates through your Twitch followers,
subscribers, viewers and latest events, with a subscriber train. **Neko Chat** shows your Twitch chat, with emotes, and takes
deleted messages off the screen again. Both come in the same 12 styles. Works on Windows and Linux.

## Install

1. Download the zip for your system from the **Releases** page on GitHub: `neko-overlays-plugin-<version>-windows-x64.zip`
   for Windows, `neko-overlays-plugin-<version>-linux-x86_64.zip` for Linux.
2. Close OBS.s
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

4. Start OBS and add a source: Sources > **+** > **Neko Info Bar** or **Neko Chat**.
5. In the source's properties, click **Connect to Twitch** (one login serves both sources). Your browser opens a page with a code. Click **Copy code**,
   then **Open Twitch**, paste the code and approve. (If the page does not open, go to `http://127.0.0.1:47831/connect`.)

Updating: close OBS and extract the new zip the same way. If the bar asks you to connect again, click **Connect to Twitch**.

## Use it

- Pick a style, colors, font and what the bar shows in the source's properties. The bar is transparent; the style draws
  its own box.
- The subscriber slot rotates through everyone who is subscribed, with their tier (for example "TheFukinMoneyBadger
  Tier 2"), a few names at a time. You are never listed yourself. A brand-new subscriber is shown right away. It uses the
  **Latest subscriber** switch in the properties.
- To see every style with made-up numbers and made-up chat, open `http://127.0.0.1:47831/` in a browser while OBS is running.
- Fonts: the styles use Google fonts. The plugin downloads each one once the first time it is needed, and after that it
  works offline. To turn this off, untick **Download Google Fonts** in the properties.

## Neko Chat

Add it like the info bar (Sources > **+** > **Neko Chat**), pick a style, and size the source in OBS (it starts at 420 x 600;
the newest message is at the bottom).

- **Deleted messages disappear.** When a moderator deletes a message, it is taken off the screen. A timeout or a ban removes
  everything that viewer said, and `/clear` empties the chat. (Messages deleted while OBS was closed or offline are of course
  not seen.)
- **Emotes.** Twitch emotes always show. **Also show 7TV, BTTV and FrankerFaceZ emotes** (on by default) adds those services'
  global and channel emotes. The pictures are loaded from their own servers, so the chat needs an internet connection for them;
  if one cannot be loaded, its name is shown instead. Badges work the same way (**Show badges**).
- **Hidden users.** Names in the **Hidden users** box never show up in the chat. It starts with `streamelements` and `streamlabs`.
  Add more, one name per line (capitals and a leading `@` do not matter), for example `nightbot` or `fossabot`. Changing the box
  applies at once. The box is per source, so each Neko Chat source has its own list.
- **Messages on screen** limits how many are shown (the oldest are pushed out). **Hide a message after this many seconds** is
  off at 0. **Use the viewers' Twitch name colors** can be switched off to use the style's own name color.
- THE FINALS shows the line as `<sub badge> [role] <name> : <text>` with the roles mod, vip, lead mod and artist, all in team blue.
  The other styles show the badge pictures.

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
| Neko Chat messages         | Chat (read only): messages, emote positions, badges, and the notices that delete them                 |

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
