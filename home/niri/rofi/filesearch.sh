#!/usr/bin/env bash
# Rofi script mode: fuzzy file search over $HOME backed by fd.
#
# Rows are two lines: the file NAME, then the parent dir below it, smaller and
# dimmed (Pango markup). The line break is U+2029 PARAGRAPH SEPARATOR rather
# than \n, because the script-mode protocol is line-delimited -- a real newline
# would be read as the start of a new entry. U+2028 LINE SEPARATOR also breaks
# the line, but it gets swallowed when Pango ellipsizes, collapsing the row back
# to one line; U+2029 survives.
#
# `listview { lines: N }` counts rows, not text lines, so the window grows to fit
# automatically. Row height is measured once at startup from a single-line
# sample, so `eh: 2` (below) is required to reserve space for the second line.
#
# The full path travels out-of-band:
#   - the raw entry is the absolute path, so rofi matches against it and
#     passes it back verbatim when the row is accepted;
#   - the `display` row option carries the markup that is actually drawn;
#   - `meta` keeps the "~/dir/name" form searchable (e.g. "~obsidian").
# Hidden files are excluded by default (fd skips dotfiles without --hidden,
# and respects .gitignore, so node_modules/target/etc. stay out of the way).
#
# Row formatting is done in awk rather than a bash `while read` loop: at ~20k
# entries the shell loop cost ~1s of pure startup latency, since rofi blocks on
# this script's stdout before it can map its window.

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
        # \342\200\251 == U+2029 PARAGRAPH SEPARATOR (see header).
        printf "%cdisplay%c%s\342\200\251<span size=\"80%%\" alpha=\"50%%\">%s/</span>", \
            0, 31, name, dir
        # thumbnail:// makes rofi render an XDG thumbnail in the icon slot
        # (falls back to the mimetype icon when no thumbnailer exists).
        printf "%cicon%cthumbnail://%s", 0, 31, path
        printf "%cmeta%c%s/%s\n", 0, 31, dir, name
    }'
