#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
    printf 'Usage: %s /path/to/Seyir.app /path/to/Seyir.dmg\n' "$0" >&2
    exit 2
fi

app=$1
output=$2
codesign --verify --strict "$app"

staging=$(mktemp -d "${TMPDIR:-/tmp}/seyir-dmg.XXXXXX")
trap 'rm -rf "$staging"' EXIT
mkdir "$staging/contents"
ditto "$app" "$staging/contents/Seyir.app"
ln -s /Applications "$staging/contents/Applications"

hdiutil create -volname Seyir -srcfolder "$staging/contents" -fs HFS+ -format UDZO "$staging/Seyir.dmg"
hdiutil verify "$staging/Seyir.dmg"
mv -f "$staging/Seyir.dmg" "$output"
