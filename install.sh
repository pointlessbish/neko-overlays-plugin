#!/usr/bin/env bash
# Location: neko-overlays-plugin/install.sh
# File: install.sh
#
# Builds the plugin and installs it for the Flatpak version of OBS.
# The Twitch Client ID is read from .env by build.rs (NEKO_TWITCH_CLIENT_ID).
# Usage: bash install.sh

set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

dest="$HOME/.var/app/com.obsproject.Studio/config/obs-studio/plugins/neko-overlays-plugin/bin/64bit"
mkdir -p "$dest"
cp target/release/libneko_obs.so "$dest/neko-overlays-plugin.so"

echo "Installed to $dest/neko-overlays-plugin.so"
echo "Restart OBS, then add the source: Sources > + > Neko Info Bar"