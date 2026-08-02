# Rofi: application launcher / dmenu (replacing walker incrementally)
#
{ pkgs, config, ... }:
{
  home.packages = with pkgs; [
    rofi
    rofi-rbw
    rofi-vpn
    rofi-network-manager
    rofi-bluetooth

    (writeShellScriptBin "rofi-preview" (builtins.readFile ./preview.sh))
    bat
    ansifilter
    imagemagick
    poppler-utils
    ffmpegthumbnailer
    file

    # Instant file search (see files-locate.sh + the updatedb-home timer).
    (writeShellScriptBin "rofi-files" (builtins.readFile ./files-locate.sh))
    plocate
  ];

  # Index all of $HOME (including the rclone GoogleDrive FUSE mount) 
  systemd.user.services.updatedb-home = {
    Unit.Description = "Index home directory for plocate";
    Service = {
      Type = "oneshot";
      Nice = 19;
      ExecStart = "${pkgs.plocate}/bin/updatedb -l 0 -o %h/.local/share/plocate/home.db -U %h --prunefs ''";
    };
  };
  systemd.user.timers.updatedb-home = {
    Unit.Description = "Update home plocate index";
    Timer = {
      OnBootSec = "5min";
      OnUnitActiveSec = "1h";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  xdg.configFile."rofi/config.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/config.rasi";
  xdg.configFile."rofi/cyberpunk.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/cyberpunk.rasi";
  xdg.configFile."rofi/filepreview.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/filepreview.rasi";
}
