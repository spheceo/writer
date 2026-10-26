#!/usr/bin/env bash
# Fetches qjs (QuickJS-ng) for the packaged app: yt-dlp's JavaScript runtime,
# so YouTube extraction works on a Mac with nothing installed. It is ~1 MB,
# so unlike deno it ships inside Writer.app (scripts/package-app.sh).
# The builds are QuickJS-ng's own macOS ones from their GitHub releases.
# They are static: the check below refuses any that would need a library a
# Mac doesn't ship with.
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
  x86_64-apple-darwin) arch=x86_64 ;;
  *) echo "No qjs for $triple" >&2; exit 1 ;;
esac

out="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
mkdir -p "$out"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Pinned: one release (0.17.0) and its SHA-256 per arch, so what's signed
# into Writer.app is what was checked. To move on, take the new release's
# qjs-darwin-<arch> assets and their hashes.
version=0.17.0
case "$arch" in
  arm64) sum=8be3ddfe3397d2e692e4e1e8972ee9d032a0a580505d2f8b4ea528cf1b651c11 ;;
  x86_64) sum=9e5e101b4fd13cda3204222ca9f8be35412c41dcdef3745829633b7a67245412 ;;
esac
url="https://github.com/quickjs-ng/quickjs/releases/download/v$version/qjs-darwin-$arch"
echo "Fetching qjs from $url"
curl -fsSL "$url" -o "$tmp/qjs"
echo "$sum  $tmp/qjs" | shasum -a 256 -c - >/dev/null || { echo "qjs's download doesn't match its pinned SHA-256." >&2; exit 1; }
if otool -L "$tmp/qjs" | tail -n +2 | grep -vE '^\s*(/usr/lib/|/System/Library/)'; then
  echo "qjs needs libraries outside macOS, so it can't be bundled." >&2
  exit 1
fi
install -m 755 "$tmp/qjs" "$out/qjs-$triple"
# Only a binary for this Mac can be run to check it.
if [ "$(lipo -archs "$out/qjs-$triple" 2>/dev/null)" = "$(uname -m)" ]; then
  "$out/qjs-$triple" --version
fi
echo "Saved to $out/qjs-$triple"
