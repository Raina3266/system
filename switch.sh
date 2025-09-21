#! /usr/bin/env bash
set -euxo pipefail

git add -A

if test -f /etc/NIXOS; then
    sudo nixos-rebuild switch --flake .#thinkpad --option eval-cache false --show-trace
else
    nix run home-manager/master -- switch --flake .
fi

# useful for debugging the above command
# --option eval-cache false --show-trace
