#!/usr/bin/env bash
# Writes Casks/rodtty.rb and Formula/rodtty.rb for the Homebrew tap.
#
# Usage: scripts/package/homebrew.sh <version> <dist-dir> <tap-dir> <notarized:true|false>
set -euo pipefail

version=${1:?version required}
dist=${2:?dist dir required}
tap=${3:?tap dir required}
notarized=${4:?notarized flag required}

root=$(cd "$(dirname "$0")/../.." && pwd)
sha() { sha256sum "$dist/$1" | cut -d' ' -f1; }

caveats=""
if [[ $notarized != true ]]; then
  caveats='
  caveats <<~EOS
    This build is not notarized by Apple. If macOS refuses to open Rodtty, run:
      xattr -dr com.apple.quarantine /Applications/Rodtty.app
  EOS
'
fi

mkdir -p "$tap/Casks" "$tap/Formula"
VERSION=$version SHA=$(sha "Rodtty-$version-macos-universal.zip") CAVEATS=$caveats \
  ruby -e 't = File.read(ARGV[0]); t.sub!("@VERSION@", ENV["VERSION"]); t.sub!("@SHA_MACOS@", ENV["SHA"]); t.sub!("@CAVEATS@", ENV["CAVEATS"]); print t' \
  "$root/packaging/homebrew/rodtty.cask.rb" > "$tap/Casks/rodtty.rb"
sed -e "s/@VERSION@/$version/" \
    -e "s/@SHA_LINUX_X86_64@/$(sha "rodtty-$version-linux-x86_64.tar.gz")/" \
    -e "s/@SHA_LINUX_AARCH64@/$(sha "rodtty-$version-linux-aarch64.tar.gz")/" \
    "$root/packaging/homebrew/rodtty.formula.rb" > "$tap/Formula/rodtty.rb"
echo "$tap/Casks/rodtty.rb"
echo "$tap/Formula/rodtty.rb"
