#!/bin/bash
# Install Tasks, a task manager for Omarchy.
#
# From a clone, ./install.sh builds from source and installs to ~/.local.
#
# Options:
#   --bind     also print a Hyprland binding to paste into ~/.config/hypr/bindings.lua
set -euo pipefail

REPO="design-nexus/nexus-tasks"

bind=false
for arg in "$@"; do
  case "$arg" in
    --bind) bind=true ;;
    -h | --help)
      sed -n '2,7p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

say() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m::\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m::\033[0m %s\n' "$*" >&2; exit 1; }

command -v hyprctl >/dev/null || warn "Hyprland wasn't found. Tasks is made for Omarchy (Hyprland), but runs elsewhere too."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

script_dir=""
if [[ -n ${BASH_SOURCE[0]:-} && -f ${BASH_SOURCE[0]} ]]; then
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

src="$script_dir"
if [[ -z $src || ! -f $src/Cargo.toml ]]; then
  command -v git >/dev/null || die "git is needed to fetch the source."
  git clone --depth 1 "https://github.com/$REPO.git" "$work/src"
  src="$work/src"
fi

if ! command -v cargo >/dev/null; then
  if command -v pacman >/dev/null; then
    say "Installing Rust and GTK 4 build dependencies"
    sudo pacman -S --needed --noconfirm rust gtk4 pkgconf gcc
  else
    die "Rust (cargo) is needed to build Tasks. Install it and run this again."
  fi
fi

say "Building Tasks (a minute or two the first time)"
(cd "$src" && cargo build --release --locked)

bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"

say "Installing to ~/.local"
install -m 755 "$src/target/release/tasks" "$bin/tasks"
install -m 644 "$src/data/io.github.design_nexus.Tasks.desktop" "$apps/"
install -m 644 "$src/data/io.github.design_nexus.Tasks.svg" "$icons/"
update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

for tool in iw; do
  command -v "$tool" >/dev/null || warn "Optional: '$tool' isn't installed; some details will be missing."
done

case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH; launch Tasks from the app launcher, or add it to PATH." ;;
esac

if [[ $bind == true ]]; then
  say "To open Tasks with Ctrl+Shift+Esc, add this to ~/.config/hypr/bindings.lua:"
  echo '  o.bind("CTRL + SHIFT + Escape", "Task manager", hl.dsp.exec_cmd("tasks --toggle"))'
fi

say "Done. Open Tasks from the app launcher, or run: tasks"
