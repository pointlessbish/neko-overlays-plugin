# neko-overlays-plugin

An OBS plugin that adds three sources. **Neko Info Bar** is a bar on your stream that rotates through your Twitch followers,
subscribers, viewers and latest events, with a subscriber train. **Neko Chat** shows your Twitch chat, with emotes, and takes
deleted messages off the screen again. **Neko Alerts** pops up a card for each new follower, subscriber, gifted sub, cheer
and raid. All three come in the same 12 styles. Works on Windows and Linux.

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

4. Start OBS and add a source: Sources > **+** > **Neko Info Bar**, **Neko Chat** or **Neko Alerts**.
5. In the source's properties, click **Connect to Twitch** (one login serves all the sources). Your browser opens a page with a code. Click **Copy code**,
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

## Neko Alerts

Add it like the others (Sources > **+** > **Neko Alerts**), pick a style, and put it where you want the cards to appear. It
starts at 700 x 400; the card is shown in the middle of the source. Alerts are shown one at a time, in the order they
happen, so a burst of events queues up instead of overlapping.

- **THE FINALS style** is the same alert as in the web version: no plate, one line of big white italic capitals like the
  game's "ELIMINATED" banner, a short white dash above it and a small yellow caption under it ("New fan", "Sponsor", "Cash out
  - 500", "Reinforcements + 42"), with a wipe in and out. The viewer's name is in their own Twitch chat colour (orange when they
    have none; a follower's colour is looked up from Twitch), and numbers and messages are in the yellow. "Names and highlights"
    in the colour options changes that yellow. Gifted subs have no counterpart in the web version, so they got "Gifted sponsor"
    and "Sponsors + 5". Needs the Barlow Condensed font, which the plugin downloads once like the other fonts.
- **Test buttons.** The source's properties have a button for each kind of alert (follower, subscriber, resub with a
  message, gifted sub, five gifted subs, cheer, raid), so you can see and place the card without waiting for a real event.
  Test alerts are shown by every Neko Alerts source that is open.
- **What gets an alert.** New followers; new subscriptions and resubs (with the viewer's message, if they wrote one); a gifted
  sub, or one card for a whole mass gift ("Gifted 5 subs") instead of five cards; cheers; and raids. Each kind has its own
  switch. **Smallest cheer** and **Smallest raid** keep small ones off the screen. **Show the viewer's message** turns the
  message line on cheers and subs off.
- **Show each alert for this many seconds** is the time the card stays up (2 to 60). The entrance and exit take a moment extra.
  Every alert type (follower, subscription or resub, gifted subs, cheer, raid) also has its own **show for this many
  seconds** box next to its picture and sound boxes (1 to 60). At 0 the type uses the general time, so you only set the ones
  that should differ, for example a short follower card and a long raid card. A video still keeps its card up until it has
  ended, if that is longer.
- **Only live events.** Opening the source (or restarting OBS) does not replay old events. Events that happened while OBS was
  closed are not shown. An alert that waited in the queue for more than five minutes is dropped.
- **Followers** are found by polling (see below), so a follow alert can come up to 15 seconds late. If several people follow
  within one poll, each gets a card (at most five at a time). The first poll after connecting only notes who the newest
  followers are, so connecting does not trigger a card for each of them. Someone who unfollows and follows again in the same
  OBS session does not get a second card.
- **Your own pictures, GIFs, videos and sounds, per alert type.** In the properties, every alert type (follower,
  subscription or resub, gifted subs, cheer, raid) has a **picture, GIF or video** box and a **sound** box. Click Browse and
  pick a file; leave a box empty for none. Pictures: png, jpg, gif, webp, apng, avif, svg. Videos: mp4, webm, ogv. Sounds:
  mp3, ogg, wav, m4a, aac, flac. A file may be at most 100 MB. Resubs use the subscription files, and a mass gift uses the
  gifted subs files.
  - The picture or video is shown at the top of the card, above the text. **Also show the text card** off shows only the
    picture or video (for an alert type without a picture or video, the text card is shown anyway). Make the source big
    enough for it: a tall picture is scaled down to fit.
  - A video keeps the card up until it has ended, if that is longer than the alert time (at most 2 minutes). A GIF plays
    over the normal alert time. Videos play their own sound too.
  - **Sound and video volume** sets the level for both.
  - Test buttons use your files, so you can check them without waiting for a real event.
  - The plugin only serves the files you picked in that source's properties; the page cannot ask for any other file.
  - **Sound reaches the stream only through your computer's audio.** The sound is played by the source's browser page on
    your default output, not through the source's own audio track in OBS. It is on the stream if your scene captures Desktop
    Audio (the usual Desktop Audio device); if you capture only specific apps, add the OBS browser audio there. It also means
    you hear the alerts yourself. Mixing it into the source's own track (so it shows in OBS's mixer) is a possible later step.

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
| Alerts: follower           | The same follower poll, newest 10 each time; names not seen before in this session get a card         |
| Alerts: sub, resub, gift   | Chat: sub, resub, subgift and submysterygift notices (a gift that is part of a mass gift has no card) |
| Alerts: cheer              | Chat: messages with bits                                                                              |
| Alerts: raid               | Chat: raid notice (the channel gets it when someone raids you)                                        |

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
