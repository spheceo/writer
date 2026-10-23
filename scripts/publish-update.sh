#!/usr/bin/env bash
# What installed Writers update from, in the cloud: the repo is private, so
# apps never read GitHub. Each build's update zip goes to R2 under
# writer/releases/<tag>/, and writer/releases/index.json lists every published
# build (src-tauri/src/update.rs reads it). Both through writer-relay, with the
# relay's client token (WRITER_RELAY_TOKEN), as the app reaches the cloud.
#
#   scripts/publish-update.sh upload <tag> <zip> <asset.json>   (each build)
#   scripts/publish-update.sh index <tag> <asset.json…>         (once, when published)
set -euo pipefail
relay="${WRITER_RELAY_URL:-https://writer-relay.sphe-g-personal.workers.dev}"
: "${WRITER_RELAY_TOKEN:?WRITER_RELAY_TOKEN is needed to reach the cloud}"
index_key=writer/releases/index.json

# A short-lived URL for a key (the relay signs it; the token never leaves this script).
presign() {
  curl -fsS -A Writer -H "authorization: Bearer $WRITER_RELAY_TOKEN" -H 'content-type: application/json' \
    "$relay/v1/cloud/r2/url" -d "{\"key\":\"$1\",\"method\":\"$2\"}" | jq -r .url
}

case "${1:-}" in
  upload)
    tag="$2" zip="$3" asset="$4"
    name="$(basename "$zip")"
    target="${name#Writer-}"
    target="${target%.zip}"
    key="writer/releases/$tag/$name"
    curl -fsS -A Writer -T "$zip" -H 'content-type: application/zip' "$(presign "$key" PUT)"
    jq -n --arg target "$target" --arg key "$key" --argjson size "$(wc -c <"$zip" | tr -d ' ')" \
      --arg sha256 "$(shasum -a 256 "$zip" | cut -d' ' -f1)" '{target: $target, key: $key, size: $size, sha256: $sha256}' >"$asset"
    echo "uploaded $name to $key"
    ;;
  index)
    tag="$2"
    shift 2
    prerelease=false
    case "$tag" in nightly-*) prerelease=true ;; esac
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)"
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    code="$(curl -sS -A Writer -o "$work/index.json" -w '%{http_code}' "$(presign "$index_key" GET)")"
    case "$code" in
      200) ;;
      404) echo '{"releases":[]}' >"$work/index.json" ;;
      *) echo "the release index answered $code" >&2; exit 1 ;;
    esac
    assets="$(jq -s 'map({(.target): {key, size, sha256}}) | add' "$@")"
    jq --arg tag "$tag" --arg version "$version" --argjson prerelease "$prerelease" --argjson assets "$assets" \
      --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      '.releases = ([.releases[] | select(.tag != $tag)] + [{tag: $tag, version: $version, prerelease: $prerelease, publishedAt: $at, assets: $assets}])' \
      "$work/index.json" >"$work/next.json"
    curl -fsS -A Writer -T "$work/next.json" -H 'content-type: application/json' "$(presign "$index_key" PUT)"
    echo "listed $tag in $index_key ($(jq '.releases | length' "$work/next.json") builds)"
    ;;
  *)
    echo "usage: publish-update.sh upload <tag> <zip> <asset.json> | index <tag> <asset.json…>" >&2
    exit 1
    ;;
esac
