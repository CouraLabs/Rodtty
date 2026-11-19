#!/usr/bin/env bash
# Builds dist/rodtty-<version>-linux-<arch>.tar.gz from release binaries.
#
# Usage: scripts/package/linux.sh <version> <arch> <release-dir>
#   arch is x86_64 or aarch64; release-dir holds rodtty and rodtty-shell.
set -euo pipefail

version=${1:?version required}
arch=${2:?arch required}
bins=${3:?release dir required}

root=$(cd "$(dirname "$0")/../.." && pwd)
name="rodtty-$version-linux-$arch"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/$name"

mkdir -p "$stage" "$root/dist"
install -m 755 "$bins/rodtty" "$bins/rodtty-shell" "$stage/"
install -m 755 "$root/packaging/linux/install.sh" "$stage/"
install -m 644 "$root/packaging/linux/rodtty.desktop" "$stage/"
install -m 644 "$root/assets/app-icon/rodtty-256.png" "$stage/rodtty.png"
install -m 644 "$root/LICENSE" "$stage/"
tar -C "$work" -czf "$root/dist/$name.tar.gz" "$name"
echo "$root/dist/$name.tar.gz"
