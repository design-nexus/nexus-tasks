#!/bin/bash
# Remove Tasks.
#
# Options:
#   --purge   also remove its settings, and the startup items it manages
#             (~/.config/hypr/tasks.lua and the line in hyprland.lua that loads it)
set -euo pipefail

purge=false
[[ ${1:-} == "--purge" ]] && purge=true

cfg="${XDG_CONFIG_HOME:-$HOME/.config}"
say() { printf '\033[1;34m::\033[0m %s\n' "$*"; }

pkill -x tasks 2>/dev/null || true
rm -f "$HOME/.local/bin/tasks" \
  "$HOME/.local/share/applications/io.github.design_nexus.Tasks.desktop" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.design_nexus.Tasks.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/nexus-tasks"
say "Removed the app."

if [[ $purge == true ]]; then
  if [[ -f $cfg/hypr/hyprland.lua ]]; then
    sed -i '/require("hypr.tasks") -- Tasks app/d' "$cfg/hypr/hyprland.lua"
  fi
  rm -f "$cfg/hypr/tasks.lua"
  rm -rf "$cfg/nexus-tasks"
  say "Removed its settings and startup items. Apps you added to ~/.config/autostart stay."
fi
