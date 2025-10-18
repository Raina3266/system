{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.work.enable {
    home.packages = with pkgs; [
      tailscale
      slack
      docker
    ];
  };
}