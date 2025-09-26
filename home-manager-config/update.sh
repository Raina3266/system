#! /usr/bin/env bash
set -euxo pipefail

nix flake update
./switch.sh