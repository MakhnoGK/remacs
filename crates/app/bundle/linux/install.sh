#!/bin/sh
# Installs remacs from this folder under a prefix, ~/.local by default: the app in
# lib/remacs, the `remacs` launcher in bin, its menu entry and icons in share.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix=${1:-"$HOME/.local"}
id=io.github.makhnogk.remacs

mkdir -p "$prefix/bin" "$prefix/lib" "$prefix/share/applications" "$prefix/share/icons"
# Copied next to its place first, so the remacs that runs is never half replaced.
rm -rf "$prefix/lib/remacs.new"
cp -R "$here/lib/remacs" "$prefix/lib/remacs.new"
rm -rf "$prefix/lib/remacs"
mv "$prefix/lib/remacs.new" "$prefix/lib/remacs"
cp "$here/bin/remacs" "$prefix/bin/remacs"
cp -R "$here/share/icons/hicolor" "$prefix/share/icons/"
# The desktop's session may not have the prefix's bin on its PATH.
sed "s|^Exec=remacs |Exec=$prefix/lib/remacs/remacs |" \
    "$here/share/applications/$id.desktop" >"$prefix/share/applications/$id.desktop"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$prefix/share/applications" || true
fi
echo "installed remacs in $prefix/lib/remacs; \`remacs\` starts it: $prefix/bin/remacs"
