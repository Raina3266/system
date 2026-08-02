#!/usr/bin/env bash
# Thumbnail generator for rofi's `-preview-cmd`.
#
# rofi calls this as: preview.sh <input> <output> <size>
# and expects a PNG written to <output>. It is only invoked for entries tagged
# `thumbnail://` (the built-in filebrowser/recursivebrowser modes tag every
# regular file), and only when <output> does not already exist -- rofi caches
# by md5 of the file path under ~/.cache/thumbnails/{normal,large,x-large}.
#
# Text files are the interesting case: there is no way to put real text in the
# preview pane, so we render syntax-highlighted text to an image via
# bat -> Pango markup -> ImageMagick.

set -uo pipefail

input=$1
output=$2
size=$3

bg="#12101A"
fg="#CBE3E7"

# Characters that fit across `size` pixels at 10pt monospace (~8px advance).
cols=$((size / 8))
((cols < 40)) && cols=40

# Lines that fit down `size * 1.3` pixels at ~17px line height.
rows=$((size * 13 / 10 / 17))
((rows < 10)) && rows=10

mime=$(file --mime-type -b -- "$input" 2>/dev/null) || mime="application/octet-stream"

render_text() {
    local markup
    markup=$(mktemp) || return 1
    # shellcheck disable=SC2064
    trap "rm -f '$markup'" RETURN

    bat --color=always --style=plain --paging=never \
        --terminal-width="$cols" --line-range=":$rows" -- "$input" 2>/dev/null |
        ansifilter --pango --font "JetBrains Mono" --font-size 10 >"$markup" || return 1

    [[ -s $markup ]] || return 1

    magick -background "$bg" -fill "$fg" -density 96 -size "${size}x" \
        "pango:@$markup" \
        -bordercolor "$bg" -border 8 \
        -resize "${size}x$((size * 3 / 2))>" \
        "PNG:$output"
}

case $mime in
    image/*)
        # [0] selects the first frame/page of animated or multi-layer images.
        magick "${input}[0]" -auto-orient -thumbnail "${size}x${size}>" "PNG:$output"
        ;;
    application/pdf)
        pdftoppm -png -f 1 -l 1 -scale-to "$size" -singlefile -- "$input" "${output%.png}"
        ;;
    video/*)
        ffmpegthumbnailer -i "$input" -o "$output" -s "$size" -c png
        ;;
    inode/x-empty)
        exit 1
        ;;
    text/* | application/json | application/xml | application/javascript | \
    application/x-shellscript | application/toml | application/x-yaml)
        render_text
        ;;
    *)
        # Anything else: only worth rendering if it is actually textual.
        if grep -Iq . -- "$input" 2>/dev/null; then
            render_text
        else
            exit 1
        fi
        ;;
esac

# Exit non-zero if nothing was produced, so rofi falls back to the mime icon.
[[ -s $output ]]
