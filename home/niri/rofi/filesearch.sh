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

set -euo pipefail

home=$HOME

if (( $# > 0 )); then
    # Selection: rofi passes the raw entry, which is the full path.
    setsid -f xdg-open "$1" >/dev/null 2>&1
    exit 0
fi

printf '\0markup-rows\x1ftrue\n'

fd --one-file-system --type f --absolute-path --base-directory "$home" . |
while IFS= read -r path; do
    name=${path##*/}
    parent=${path%/*}
    dir=${parent#"$home"}
    dir="~${dir}"
    printf '%s\0display\x1f<span size="small" alpha="55%%">%s/</span>%s\x1fmeta\x1f%s/%s\n' \
        "$path" "$dir" "$name" "$dir" "$name"
done
