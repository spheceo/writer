#!/usr/bin/env bash
# Fetches static ffmpeg and ffprobe for the packaged app, so nobody installs
# them. The builds are Martin Riedl's macOS ones, linked from ffmpeg.org's
# download page. They are static: the check below refuses any that would need
# a library a Mac doesn't ship with.
set -euo pipefail

case "$(uname -m)" in
  arm64) triple=aarch64-apple-darwin; arch=arm64 ;;
  x86_64) triple=x86_64-apple-darwin; arch=amd64 ;;
  *) echo "Unknown Mac architecture: $(uname -m)" >&2; exit 1 ;;
esac

out="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
mkdir -p "$out"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

for tool in ffmpeg ffprobe; do
  url="https://ffmpeg.martin-riedl.de/redirect/latest/macos/$arch/release/$tool.zip"
  echo "Fetching $tool from $url"
  curl -fsSL "$url" -o "$tmp/$tool.zip"
  ditto -x -k "$tmp/$tool.zip" "$tmp/$tool"
  found="$(find "$tmp/$tool" -type f -name "$tool" -perm -u+x | head -1)"
  [ -n "$found" ] || { echo "No $tool in the download." >&2; exit 1; }
  if otool -L "$found" | tail -n +2 | grep -vE '^\s*(/usr/lib/|/System/Library/)'; then
    echo "$tool needs libraries outside macOS, so it can't be bundled." >&2
    exit 1
  fi
  install -m 755 "$found" "$out/$tool-$triple"
  "$out/$tool-$triple" -version | head -1
done
echo "Saved to $out"
