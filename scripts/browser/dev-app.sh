#!/usr/bin/env bash
# Builds target/browser/Rodtty.app: a development bundle of the debug build
# with the embedded browser, ad-hoc signed. CEF on macOS loads its framework
# and helpers relative to the app bundle, so browser runs need this layout.
#
# Usage: CEF_PATH=~/.local/share/cef scripts/browser/dev-app.sh [--release]
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
profile=debug
flags=()
if [[ ${1:-} == --release ]]; then profile=release; flags=(--release); fi
cd "$root"
cargo build ${flags[@]+"${flags[@]}"} --features browser --bins
bins="$root/target/$profile"
app="$root/target/browser/Rodtty.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
sed -e "s/@VERSION@/$version/g" -e "s/@MINIMUM_SYSTEM_VERSION@/11.0/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp assets/app-icon/Rodtty.icns "$app/Contents/Resources/"
cp "$bins/rodtty" "$bins/rodtty-shell" "$app/Contents/MacOS/"
scripts/package/macos-cef.sh "$app" "$bins"
for helper in "$app/Contents/Frameworks/"*Helper*.app; do codesign --force --sign - "$helper"; done
codesign --force --sign - "$app/Contents/Frameworks/Chromium Embedded Framework.framework"
codesign --force --sign - "$app/Contents/MacOS/rodtty-shell"
codesign --force --sign - "$app/Contents/MacOS/rodtty"
codesign --force --sign - "$app"
echo "$app"
