#!/bin/zsh
# Writer.app for a target, then what ships of it: the update zip (what an
# installed Writer updates from) and the drag-to-Applications disk image.
#
#   scripts/package-app.sh <target> <out dir>
#
# Writes <out>/Writer-<target>.zip (+ .sha256) and
# <out>/Writer-<Apple-Silicon|Intel>.dmg. The build's name comes from
# WRITER_BUILD ("v0.0.0", "nightly-1") and WRITER_RELEASE=1 turns on
# self-update; WRITER_RELAY_TOKEN is built in (src-tauri/src/settings.rs).
#
# Signing: with MACOS_CERTIFICATE (a Developer ID Application .p12, base64) and
# MACOS_CERTIFICATE_PASSWORD, every program in the app is signed with it under
# the hardened runtime; with NOTARY_KEY (an App Store Connect API key .p8,
# base64), NOTARY_KEY_ID and NOTARY_ISSUER too, the disk image is notarised
# and the ticket stapled to it and the app. Without them, it's all ad hoc.
set -euo pipefail
here=${0:A:h}
root=${here:h}
target=${1:?usage: package-app.sh <target> <out dir>}
out=${2:?usage: package-app.sh <target> <out dir>}
mkdir -p "$out"
out=${out:A}
case "$target" in
  aarch64-apple-darwin) human=Apple-Silicon ;;
  x86_64-apple-darwin) human=Intel ;;
  *) echo "no Mac app for $target" >&2; exit 1 ;;
esac
work=$(mktemp -d)
# One path per entry, spaces kept ("Application Support").
keychains=("${(@f)$(security list-keychains -d user | sed -E 's/^[[:space:]]*"(.*)"$/\1/')}")
# Puts the search list back as it was (on a Mac, not just CI) and removes the signing keychain.
trap 'rm -rf "$work"; if [[ -n "${keychain:-}" ]]; then security list-keychains -d user -s "${keychains[@]}"; security delete-keychain "$keychain" 2>/dev/null; fi; true' EXIT

echo "==> ffmpeg and ffprobe ($target)"
[[ -x "$root/src-tauri/binaries/ffmpeg-$target" && -x "$root/src-tauri/binaries/ffprobe-$target" ]] || "$root/scripts/fetch-ffmpeg.sh" "$target"

echo "==> Writer.app ($target, ${WRITER_BUILD:-dev})"
(cd "$root" && bunx @tauri-apps/cli@2 build --target "$target" --bundles app --config src-tauri/tauri.bundle.conf.json)
built="${CARGO_TARGET_DIR:-$root/src-tauri/target}/$target/release/bundle/macos/Writer.app"
app="$work/Writer.app"
ditto "$built" "$app"

identity=-
runtime=()
if [[ -n "${MACOS_CERTIFICATE:-}" ]]; then
  echo "==> Signing with the Developer ID"
  keychain="$work/signing.keychain-db"
  # No pipes cut short here: under pipefail a reader that stops early fails the script (SIGPIPE, 141).
  password=$(openssl rand -hex 16)
  print -rn -- "$MACOS_CERTIFICATE" | base64 --decode >"$work/certificate.p12"
  security create-keychain -p "$password" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$password" "$keychain"
  security import "$work/certificate.p12" -k "$keychain" -P "${MACOS_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
  security list-keychains -d user -s "$keychain" "${keychains[@]}"
  identity=$(security find-identity -v -p codesigning "$keychain" | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | sed -n 1p)
  [[ -n "$identity" ]] || { echo "no Developer ID Application identity in MACOS_CERTIFICATE" >&2; exit 1; }
  runtime=(--options runtime --timestamp)
fi
# Inside out: the bundled programs (ffmpeg, ffprobe beside Writer), then the app (its executable and seal).
for program in "$app"/Contents/MacOS/*(.); do
  [[ "${program:t}" == Writer ]] && continue
  codesign --force "${runtime[@]}" -s "$identity" "$program"
done
codesign --force "${runtime[@]}" -s "$identity" --entitlements "$here/writer.entitlements" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

echo "==> Writer-$human.dmg"
dmg="$out/Writer-$human.dmg"
stage="$work/dmg"
mkdir "$stage"
ditto "$app" "$stage/Writer.app"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -quiet -volname Writer -srcfolder "$stage" -fs HFS+ -format UDZO "$dmg"
if [[ "$identity" != - ]]; then
  codesign --force --timestamp -s "$identity" "$dmg"
  if [[ -n "${NOTARY_KEY:-}" && -n "${NOTARY_KEY_ID:-}" && -n "${NOTARY_ISSUER:-}" ]]; then
    echo "==> Notarising (a few minutes)"
    print -rn -- "$NOTARY_KEY" | base64 --decode >"$work/notary.p8"
    xcrun notarytool submit "$dmg" --key "$work/notary.p8" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" --wait
    xcrun stapler staple "$dmg"
    # The image's ticket covers the app inside it: staple it to the app the updater downloads too.
    xcrun stapler staple "$app"
  fi
fi

zip="$out/Writer-$target.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
(cd "$out" && shasum -a 256 "${zip:t}" >"${zip:t}.sha256" && shasum -a 256 "${dmg:t}" >"${dmg:t}.sha256")
echo "==> ${zip:t}, ${dmg:t}"
