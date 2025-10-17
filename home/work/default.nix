{
  lib,
  pkgs,
  config,
  ...
}:
{
  options = {
    work.enable = lib.mkEnableOption "work stuff";
  };
  config = lib.mkIf config.work.enable {
    home.packages = with pkgs; [
      tailscale
      slack
    ];
  };
}