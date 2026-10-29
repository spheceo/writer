set shell := ["bash", "-cu"]

# Desktop app, with Vite hot reload
dev:
    mkdir -p dist
    bun run tauri dev

# Typecheck, clip tests, and the Rust tests
check:
    bun install --cwd relay --frozen-lockfile --silent
    bun install --cwd sidecar --frozen-lockfile --silent
    node_modules/.bin/tsc -p sidecar
    bun run typecheck
    bun test
    cargo test --manifest-path src-tauri/Cargo.toml

# Local relay. Its keys are Worker secrets (from Infisical), never an env file.
relay:
    bunx wrangler dev --cwd relay

# Put the relay's secrets on the Worker from Infisical (/writer-relay), printing names only: --yes to send, --only A,B
relay-secrets *args:
    bun scripts/push-relay-secrets.ts {{args}}

# The app to hand out: ffmpeg and ffprobe inside it, and the relay's address
# built in, so users install only Claude Code, sign in, and never see the relay.
bundle:
    test -x "src-tauri/binaries/ffmpeg-$(rustc -vV | sed -n 's/host: //p')" || ./scripts/fetch-ffmpeg.sh
    test -x "src-tauri/binaries/qjs-$(rustc -vV | sed -n 's/host: //p')" || ./scripts/fetch-quickjs.sh
    bun run tauri build --config src-tauri/tauri.bundle.conf.json

# Releases go up a patch at a time (v0.0.0, v0.0.1, …); in between, nightlies
# (nightly-1, nightly-2, …). GitHub Actions (.github/workflows/release.yml)
# builds Writer.app for Apple Silicon and Intel from the tag and publishes it.
# Only when the user says to make a new version; otherwise ship a nightly.
# Ship the next patch version (the current one if it was never released): check, bump, tag, push (CI builds and publishes)
publish: check
    #!/usr/bin/env bash
    set -euo pipefail
    [ "$(git branch --show-current)" = "main" ] || { echo "publish from main"; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "commit your changes first"; exit 1; }
    git fetch -q --tags origin
    current="$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)"
    if ! git rev-parse -q --verify "refs/tags/v$current" >/dev/null && ! gh release view "v$current" >/dev/null 2>&1; then
        # Never released (v0.0.0, the first): release it as it is.
        git tag "v$current"
        git push origin main "v$current"
        echo "Pushed v$current. GitHub Actions builds and publishes it (~15 min):"
        echo "  https://github.com/spheceo/writer/actions/workflows/release.yml"
        exit 0
    fi
    IFS=. read -r major minor patch <<<"$current"
    version="$major.$minor.$((patch + 1))"
    tag="v$version"
    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null || gh release view "$tag" >/dev/null 2>&1; then
        echo "$tag already exists"
        exit 1
    fi
    # Put the version back if anything fails before the release commit.
    trap 'git checkout -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json package.json' ERR
    sed -i '' "s/^version = \"$current\"/version = \"$version\"/" src-tauri/Cargo.toml
    sed -i '' "s/\"version\": \"$current\"/\"version\": \"$version\"/" src-tauri/tauri.conf.json package.json
    (cd src-tauri && cargo update -p writer -q)
    git commit -q -m "Release $tag" src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json package.json
    trap - ERR
    git tag "$tag"
    git push origin main "$tag"
    echo "Pushed $tag. GitHub Actions builds and publishes it (~15 min):"
    echo "  https://github.com/spheceo/writer/actions/workflows/release.yml"

# CI builds the tag and publishes it as a pre-release; installs on the Nightly
# channel (the default) offer it.
# Ship a nightly: tag main's head nightly-<N> (the next number) and push
nightly:
    #!/usr/bin/env bash
    set -euo pipefail
    [ "$(git branch --show-current)" = "main" ] || { echo "publish from main"; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "commit your changes first"; exit 1; }
    git fetch -q --tags origin
    last="$(git tag --list 'nightly-*' | sed 's/^nightly-//' | sort -n | tail -1)"
    tag="nightly-$(( ${last:-0} + 1 ))"
    git tag "$tag"
    git push origin main "$tag"
    echo "Pushed $tag. GitHub Actions builds and publishes it (~15 min):"
    echo "  https://github.com/spheceo/writer/actions/workflows/release.yml"
