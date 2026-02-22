{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
    ./jellyfin.nix
    ./virtual.nix
  #  ./cloud.nix
  ];
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      onedrivegui
      wechat
      qq
      wemeet
      ytdownloader
      wasistlos
      inkscape
      shotcut
      qbittorrent
      android-tools
    ];
  };
}
