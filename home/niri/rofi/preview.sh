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
#
# For anything with no meaningful preview (binaries, archives, empty files) we
# write a 1x1 transparent PNG rather than failing. Rofi cannot hide the preview
# widget at runtime, but with `squared: false` in the theme the widget's desired
# width is derived from the image aspect ratio, so a 1px-wide image makes the
# pane collapse and the listview reclaims the space.

set -uo pipefail

input=$1
output=$2
size=$3

bg="#12101A"
fg="#CBE3E7"

# Characters that fit across `size` pixels at 9pt monospace (~7px advance).
cols=$((size / 7))
((cols < 40)) && cols=40

# Lines that fit down `size` pixels at ~15px line height. Kept within `size`
# (rather than taller) so the rendered image never exceeds the pane's box and
# gets scaled down -- downscaling makes the text blurry and small.
rows=$((size / 15))
((rows < 10)) && rows=10

# Collapse the preview pane: a 1x1 transparent PNG.
no_preview() {
    magick -size 1x1 xc:none "PNG:$output"
    exit 0
}

mime=$(file --mime-type -b -- "$input" 2>/dev/null) || mime="application/octet-stream"

render_text() {
    local markup
    markup=$(mktemp) || return 1
    # shellcheck disable=SC2064
    trap "rm -f '$markup'" RETURN

    bat --color=always --style=plain --paging=never --wrap=character \
        --terminal-width="$cols" --line-range=":$rows" -- "$input" 2>/dev/null |
        ansifilter --pango --font "JetBrains Mono" --font-size 9 >"$markup" || return 1

    [[ -s $markup ]] || return 1

    magick -background "$bg" -fill "$fg" -density 96 -size "${size}x" \
        "pango:@$markup" \
        -bordercolor "$bg" -border 8 \
        -resize "${size}x${size}>" \
        -background "$bg" -gravity northwest -extent "${size}x${size}" \
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
    inode/x-empty | inode/directory)
        no_preview
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
            no_preview
        fi
        ;;
esac

# If a renderer failed part-way, still collapse the pane rather than leaving
# rofi to fall back to a giant mime icon.
[[ -s $output ]] || no_preview
