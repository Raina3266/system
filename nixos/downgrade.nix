{ config, pkgs, lib, nixpkgs-spotdl, ... }:

{
  nixpkgs.overlays = [
    (self: super: {
      spotdl = nixpkgs-spotdl.legacyPackages.${pkgs.system}.spotdl;
    })
  ];
}