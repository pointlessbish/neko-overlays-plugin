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
5. In the source's properties, click **Connect to Twitch**, open the address shown on the bar, and enter the code.

Updating: close OBS and extract the new zip the same way. If the bar asks you to connect again, click **Connect to Twitch**.

## Use it

- Pick a style, colors, font and what the bar shows in the source's properties. The bar is transparent; the style draws
  its own box.
- To see every style with made-up numbers, open `http://127.0.0.1:47831/` in a browser while OBS is running.
- Fonts: the styles use Google fonts. The plugin downloads each one once the first time it is needed, and after that it
  works offline. To turn this off, untick **Download Google Fonts** in the properties.

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

## Build it yourself

See [BUILD.md](BUILD.md).
