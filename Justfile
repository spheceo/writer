set shell := ["bash", "-cu"]

# Desktop app, with Vite hot reload
dev:
    mkdir -p dist
    bun run tauri dev

# Typecheck, clip tests, and the Rust tests
check:
    bun run typecheck
    bun test
    cargo test --manifest-path src-tauri/Cargo.toml

# Local relay. The ElevenLabs key is a worker secret, not an env file.
relay:
    bunx wrangler dev --cwd relay

# The app to hand out: ffmpeg and ffprobe inside it, and the relay's address and
# token built in, so users install only Claude Code and never see the relay.
# The token comes from WRITER_RELAY_TOKEN, or this Mac's Writer settings.
bundle:
    test -x "src-tauri/binaries/ffmpeg-$(rustc -vV | sed -n 's/host: //p')" || ./scripts/fetch-ffmpeg.sh
    WRITER_RELAY_TOKEN="${WRITER_RELAY_TOKEN:-$(python3 -c 'import json,os;print(json.load(open(os.path.expanduser("~/Library/Application Support/com.spheceo.writer/settings.json"))).get("relayToken",""))')}" \
      && test -n "$WRITER_RELAY_TOKEN" || { echo "Set WRITER_RELAY_TOKEN to the relay's client token." >&2; exit 1; } \
      && WRITER_RELAY_TOKEN="$WRITER_RELAY_TOKEN" bun run tauri build --config src-tauri/tauri.bundle.conf.json
