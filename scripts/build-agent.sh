#!/usr/bin/env bash
# Builds writer-agent (sidecar/: Writer's way to Claude through the Agent SDK,
# as Edit's agent) into one program for the packaged app, beside ffmpeg:
#   scripts/build-agent.sh [target triple]   (this Mac's by default)
# It runs the user's own `claude`, so the SDK's built-in Claude Code (its
# optional platform package) is never bundled.
set -euo pipefail
triple="${1:-}"
if [ -z "$triple" ]; then
  case "$(uname -m)" in
    arm64) triple=aarch64-apple-darwin ;;
    x86_64) triple=x86_64-apple-darwin ;;
    *) echo "Unknown Mac architecture: $(uname -m)" >&2; exit 1 ;;
  esac
fi
case "$triple" in
  aarch64-apple-darwin) bun_target=bun-darwin-arm64 ;;
  x86_64-apple-darwin) bun_target=bun-darwin-x64 ;;
  *) echo "No writer-agent for $triple" >&2; exit 1 ;;
esac
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/src-tauri/binaries"
mkdir -p "$out"
(cd "$root/sidecar" && bun install --frozen-lockfile --silent)
bun build "$root/sidecar/src/main.ts" --compile --minify --target "$bun_target" \
  --external @anthropic-ai/claude-agent-sdk-darwin-arm64 --external @anthropic-ai/claude-agent-sdk-darwin-x64 \
  --outfile "$out/writer-agent-$triple"
echo "==> $out/writer-agent-$triple ($(du -h "$out/writer-agent-$triple" | cut -f1))"
