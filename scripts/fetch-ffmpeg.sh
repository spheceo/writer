#!/usr/bin/env bash
# Fetches static ffmpeg and ffprobe for the packaged app, so nobody installs
# them. The builds are Martin Riedl's macOS ones, linked from ffmpeg.org's
# download page. They are static: the check below refuses any that would need
# a library a Mac doesn't ship with.
set -euo pipefail

# The target to fetch for: this Mac's by default, or the one given (the
# release workflow builds the Intel app on an Apple Silicon runner).
triple="${1:-}"
if [ -z "$triple" ]; then
  case "$(uname -m)" in
    arm64) triple=aarch64-apple-darwin ;;
    x86_64) triple=x86_64-apple-darwin ;;
    *) echo "Unknown Mac architecture: $(uname -m)" >&2; exit 1 ;;
  esac
fi
case "$triple" in
  aarch64-apple-darwin) arch=arm64 ;;
  x86_64-apple-darwin) arch=amd64 ;;
  *) echo "No ffmpeg for $triple" >&2; exit 1 ;;
esac

out="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
mkdir -p "$out"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Pinned: one build (9.0.2) and its SHA-256, so what's signed into Writer.app
# is what was checked. To move on, take the URLs `.../redirect/latest/macos/
# <arch>/release/<tool>.zip` points to, and their hashes.
pinned() {
  case "$arch/$1" in
    arm64/ffmpeg) echo "1789931890_9.0.2 c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924" ;;
    arm64/ffprobe) echo "1789931890_9.0.2 fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6" ;;
    amd64/ffmpeg) echo "1789931006_9.0.2 7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4" ;;
    amd64/ffprobe) echo "1789931006_9.0.2 2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd" ;;
  esac
}

for tool in ffmpeg ffprobe; do
  read -r build sum <<<"$(pinned "$tool")"
  url="https://ffmpeg.martin-riedl.de/download/macos/$arch/$build/$tool.zip"
  echo "Fetching $tool from $url"
  curl -fsSL "$url" -o "$tmp/$tool.zip"
  echo "$sum  $tmp/$tool.zip" | shasum -a 256 -c - >/dev/null || { echo "$tool's download doesn't match its pinned SHA-256." >&2; exit 1; }
  ditto -x -k "$tmp/$tool.zip" "$tmp/$tool"
  found="$(find "$tmp/$tool" -type f -name "$tool" -perm -u+x | head -1)"
  [ -n "$found" ] || { echo "No $tool in the download." >&2; exit 1; }
  if otool -L "$found" | tail -n +2 | grep -vE '^\s*(/usr/lib/|/System/Library/)'; then
    echo "$tool needs libraries outside macOS, so it can't be bundled." >&2
    exit 1
  fi
  install -m 755 "$found" "$out/$tool-$triple"
  # Only a binary for this Mac can be run to check it.
  if [ "$(lipo -archs "$out/$tool-$triple" 2>/dev/null)" = "$(uname -m)" ]; then
    "$out/$tool-$triple" -version | head -1
  fi
done
echo "Saved to $out"
