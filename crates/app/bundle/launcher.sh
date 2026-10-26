#!/bin/sh
# Remacs.app/Contents/Resources/launcher, linked as `remacs` by the Homebrew cask: starts
# remacs apart from the terminal, so that closing its tab does not quit it; the app sends
# what it prints to its log.
binary="$(dirname "$(readlink -f "$0")")/../MacOS/remacs"
[ "${1-}" = --version ] && exec "$binary" --version
nohup "$binary" "$@" </dev/null >/dev/null 2>&1 &
