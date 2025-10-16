{
  lib,
  pkgs,
  config,
  ...
}:
{
  imports = [
    ./cloud.nix
    ./jellyfin.nix
  ];
  options = {
    personal.enable = lib.mkEnableOption "Personal stuff";
  };
  config = lib.mkIf config.personal.enable {
    home.packages = with pkgs; [
      obsidian
      discord
      kid3
      gui-for-clash
      wechat
      qq
      ytdownloader
      waylyrics
      whatsapp-for-linux
      inkscape
      shotcut
    ];
  };
}
