#!/usr/bin/env bash
# Adds the embedded browser runtime to a Rodtty.app bundle: the Chromium
# Embedded Framework and the five helper apps CEF expects on macOS
# (Rodtty Helper, (GPU), (Renderer), (Plugin), (Alerts)), each running the
# rodtty-browser-helper executable.
#
# Usage: scripts/package/macos-cef.sh <Rodtty.app> <dir with rodtty-browser-helper>
# Requires CEF_PATH: the CEF binary distribution matching the `cef` crate
# (see docs/browser.md). Code signing is left to the caller, innermost first.
set -euo pipefail
app=${1:?app bundle required}
bins=${2:?binary dir required}
cef=${CEF_PATH:?set CEF_PATH to the CEF distribution}
root=$(cd "$(dirname "$0")/../.." && pwd)
frameworks="$app/Contents/Frameworks"
mkdir -p "$frameworks"
rm -rf "$frameworks/Chromium Embedded Framework.framework"
cp -R "$cef/Chromium Embedded Framework.framework" "$frameworks/"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
for variant in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  name="Rodtty Helper$variant"
  helper="$frameworks/$name.app"
  suffix=$(echo "$variant" | tr -d ' ()' | tr 'A-Z' 'a-z')
  id="dev.couralabs.rodtty.helper${suffix:+.$suffix}"
  rm -rf "$helper"
  mkdir -p "$helper/Contents/MacOS"
  cp "$bins/rodtty-browser-helper" "$helper/Contents/MacOS/$name"
  sed -e "s/@NAME@/$name/g" -e "s/@ID@/$id/g" -e "s/@VERSION@/$version/g" \
    "$root/packaging/macos/Helper-Info.plist" > "$helper/Contents/Info.plist"
  plutil -lint -s "$helper/Contents/Info.plist"
done
