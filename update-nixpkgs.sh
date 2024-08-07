#! /usr/bin/env sh
set -euxo pipefail

nix flake lock --update-input nixpkgs