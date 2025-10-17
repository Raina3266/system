#! /usr/bin/env bash
set -euxo pipefail

# ./switch.sh thinkpad
# ./switch.sh dell

NAME="$1"  # either "thinkpad" or "dell"

git add -A

sudo nixos-rebuild switch --flake ".#$NAME" --show-trace

# useful for debugging the above command
# --option eval-cache false --show-trace
