#!/usr/bin/env bash
# Rofi script mode: fuzzy file search over $HOME backed by fd.
#
# Rows show only the file NAME with the parent dir dimmed behind it (Pango
# markup). Hidden files are excluded by default (fd skips dotfiles without --hidden,
# and respects .gitignore, so node_modules/target/etc. stay out of the way).

set -euo pipefail

home=$HOME

if (( $# > 0 )); then
    # Selection: strip the \x1f display columns to recover the real path.
    path=${1##*$'\x1f'}
    setsid -f xdg-open "$path" >/dev/null 2>&1
    exit 0
fi

printf '\0markup-rows\x1ftrue\n'

fd --one-file-system --type f --absolute-path --base-directory "$home" . |
while IFS= read -r path; do
    name=${path##*/}
    dir=${path%/*}
    dir=${dir#"$home"/}              # strip ~ for display
    printf '%s<span size="small" alpha="55%%">  %s/</span>\x1f%s\n' \
        "$name" "$dir" "$path"
done
