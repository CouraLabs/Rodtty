#!/bin/sh
# Installs Rodtty from this extracted release archive and adds it to the app launcher.
#
# Usage: ./install.sh [--prefix DIR] [--uninstall]
#   Default prefix is ~/.local (no root needed). Use --prefix /usr/local with sudo
#   to install for every user.
set -eu

prefix="${HOME}/.local"
uninstall=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix="${2:?--prefix needs a directory}"; shift 2 ;;
    --uninstall) uninstall=1; shift ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

here=$(cd "$(dirname "$0")" && pwd)
libexec="$prefix/lib/rodtty"
apps="$prefix/share/applications"
icons="$prefix/share/icons/hicolor/256x256/apps"

if [ "$uninstall" -eq 1 ]; then
  rm -rf "$libexec"
  rm -f "$prefix/bin/rodtty" "$apps/rodtty.desktop" "$icons/rodtty.png"
  echo "Removed Rodtty from $prefix"
  exit 0
fi

# rodtty finds rodtty-shell beside its real path, so both stay in one directory
# and bin/rodtty is a symlink to it.
mkdir -p "$libexec" "$prefix/bin" "$apps" "$icons"
install -m 755 "$here/rodtty" "$here/rodtty-shell" "$libexec/"
ln -sf "$libexec/rodtty" "$prefix/bin/rodtty"
install -m 644 "$here/rodtty.png" "$icons/rodtty.png"
sed -e "s|^Exec=.*|Exec=$libexec/rodtty|" -e "s|^Icon=.*|Icon=$icons/rodtty.png|" \
  "$here/rodtty.desktop" > "$apps/rodtty.desktop"
chmod 644 "$apps/rodtty.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" 2>/dev/null || true

echo "Installed Rodtty to $libexec"
echo "Open it from your app launcher, or run: $prefix/bin/rodtty"
