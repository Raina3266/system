{ pkgs, ... }:
let
  mediaControl = pkgs.callPackage ../../../scripts/media-control/package.nix { };
in
{
  home.packages = [ mediaControl ];
}

