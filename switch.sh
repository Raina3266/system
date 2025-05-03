#! /usr/bin/env sh
set -euxo pipefail

git add -A
sudo nixos-rebuild switch --flake .#thinkpad --option eval-cache false --show-trace

# useful for debugging the above command
# --option eval-cache false --show-trace
