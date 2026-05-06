{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
    #  ./cloud.nix
  ];
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      onedrivegui
      wechat
      qq
      wemeet
      whatsie
      inkscape
      shotcut
      qbittorrent
      android-tools
      jellyfin
      jellyfin-web
    ];
    dconf.settings = {
      "org/gnome/desktop/interface" = {
        enable-hot-corners = false;
      };

      "org/gnome/desktop/interface" = {
        show-battery-percentage = true;
      };

      "org/gnome/mutter" = {
        center-new-windows = true;
      };

      # Custom keybindings
      "org/gnome/desktop/wm/keybindings" = {
        close = [ "<Super>q" ];
      };
    };
  };
}
