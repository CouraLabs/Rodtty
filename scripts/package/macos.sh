#!/usr/bin/env bash
# Builds dist/Rodtty-<version>-macos-<arch>.{zip,dmg} from release binaries.
#
# Usage: scripts/package/macos.sh <version> <release-dir> [<release-dir> ...]
#   Each release dir holds rodtty and rodtty-shell for one architecture,
#   e.g. target/aarch64-apple-darwin/release. Two dirs produce a universal app.
#
# Optional environment:
#   APPLE_SIGNING_IDENTITY  Developer ID Application identity; ad-hoc signing when unset.
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD  With a signing identity, notarize and staple.
#   MACOSX_DEPLOYMENT_TARGET  Minimum macOS version recorded in Info.plist (default 11.0).
set -euo pipefail

version=${1:?version required}
shift
[[ $# -ge 1 ]] || { echo "at least one release dir required" >&2; exit 2; }

root=$(cd "$(dirname "$0")/../.." && pwd)
dist="$root/dist"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [[ $# -gt 1 ]]; then
  arch=universal
else
  arch=$(lipo -archs "$1/rodtty")
  [[ $arch == arm64 ]] && arch=aarch64
fi
name="Rodtty-$version-macos-$arch"
app="$work/Rodtty.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$dist"
for bin in rodtty rodtty-shell; do
  inputs=()
  for dir in "$@"; do inputs+=("$dir/$bin"); done
  lipo -create "${inputs[@]}" -output "$app/Contents/MacOS/$bin"
done
sed -e "s/@VERSION@/$version/g" \
    -e "s/@MINIMUM_SYSTEM_VERSION@/${MACOSX_DEPLOYMENT_TARGET:-11.0}/g" \
    "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/assets/app-icon/Rodtty.icns" "$app/Contents/Resources/Rodtty.icns"
plutil -lint "$app/Contents/Info.plist"

identity=${APPLE_SIGNING_IDENTITY:--}
sign=(codesign --force --sign "$identity")
[[ $identity != - ]] && sign+=(--options runtime --timestamp)
# Nested code first, then the bundle that seals it.
"${sign[@]}" "$app/Contents/MacOS/rodtty-shell"
"${sign[@]}" "$app/Contents/MacOS/rodtty"
"${sign[@]}" "$app"
codesign --verify --strict --verbose=2 "$app"

if [[ $identity != - && -n ${APPLE_ID:-} && -n ${APPLE_TEAM_ID:-} && -n ${APPLE_APP_PASSWORD:-} ]]; then
  ditto -c -k --keepParent "$app" "$work/notarize.zip"
  xcrun notarytool submit "$work/notarize.zip" --wait \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD"
  xcrun stapler staple "$app"
  spctl --assess --type execute --verbose "$app"
fi

rm -f "$dist/$name.zip" "$dist/$name.dmg"
ditto -c -k --keepParent "$app" "$dist/$name.zip"
mkdir "$work/dmg"
cp -R "$app" "$work/dmg/"
ln -s /Applications "$work/dmg/Applications"
hdiutil create -volname Rodtty -srcfolder "$work/dmg" -format UDZO -ov "$dist/$name.dmg" >/dev/null
[[ $identity != - ]] && codesign --force --sign "$identity" --timestamp "$dist/$name.dmg"
echo "$dist/$name.zip"
echo "$dist/$name.dmg"
