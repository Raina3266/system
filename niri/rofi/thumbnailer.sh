#!/usr/bin/env bash
# XDG thumbnailer for application/pdf, using pdftoppm (poppler).
# No preview-cmd in rofi on purpose.
# 
# $1 = input file (%i), $2 = output png (%o), $3 = size in px (%s)

set -euo pipefail

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# First page only, scaled so the longest edge is $3.
pdftoppm -png -f 1 -singlefile -scale-to "$3" -- "$1" "$tmp/out"

[ -s "$tmp/out.png" ]
mv -- "$tmp/out.png" "$2"
