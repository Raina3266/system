{
  lib,
  nixosConfig,
  pkgs,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      jellyfin
      jellyfin-web
    ];
  };
}
