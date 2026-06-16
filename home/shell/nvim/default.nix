{ inputs, pkgs, ... }:
{
  imports = [ inputs.nixvim.homeModules.nixvim ];
  programs.nixvim = {
    enable = true;
    nixpkgs.source = pkgs.path;
  };
}
