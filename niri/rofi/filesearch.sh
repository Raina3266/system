#!/usr/bin/env bash
# Rofi script mode: fuzzy file search over $HOME via fd.
# Two-line rows (thumbnails + name + dimmed parent dir).

set -euo pipefail

home=$HOME

if (($# > 0)); then
    setsid -f xdg-open "$1" >/dev/null 2>&1
    exit 0
fi

printf '\0markup-rows\x1ftrue\n'

fd --one-file-system --type f --base-directory "$home" . |
    awk -v home="$home" '
    {
        slash = 0
        for (i = length($0); i > 0; i--) {
            if (substr($0, i, 1) == "/") { slash = i; break }
        }
        name = (slash ? substr($0, slash + 1) : $0)
        dir  = (slash ? "~/" substr($0, 1, slash - 1) : "~")
        path = home "/" $0

        printf "%s", path
        printf "%cdisplay%c%s\342\200\251<span size=\"80%%\" alpha=\"50%%\">%s/</span>", \
            0, 31, name, dir

        printf "%cicon%cthumbnail://%s,text-x-generic", 31, 31, path
        printf "%cmeta%c%s/%s\n", 31, 31, dir, name
    }'
