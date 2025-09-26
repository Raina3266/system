#! /usr/bin/env bash

git add -A
nix run home-manager/master -- switch --flake . -b backup

# nix flake update
