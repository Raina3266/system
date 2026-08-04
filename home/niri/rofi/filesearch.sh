#!/usr/bin/env bash
# Rofi script mode: fuzzy file search over $HOME backed by fd.
#
# Rows show only the file NAME with the parent dir dimmed behind it (Pango
# markup). The full path travels out-of-band:
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

# --base-directory keeps fd's output relative so awk can split the basename
# without rescanning the $HOME prefix; the absolute path is rebuilt below.
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
        printf "%cdisplay%c%s<span size=\"80%%\" alpha=\"50%%\">  %s/</span>", \
            0, 31, name, dir
        printf "%cmeta%c%s/%s\n", 0, 31, dir, name
    }'
