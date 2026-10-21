# Media and audio use AGS/Astal GTK4; the remaining panels stay in Quickshell.
{
  config,
  lib,
  pkgs,
  repoRoot,
  repoPackages,
  ...
}:
{
  home.packages = [
    repoPackages.mediaPanel
    repoPackages.mediaControl
    repoPackages.audioPanel
    repoPackages.audioControl
  ];

  # Keep generated assets outside this directory-level live link so Home
  # Manager never writes generated child links into the repository.
  xdg.configFile."ags/media".source =
    config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/media-control/astal";
  xdg.configFile."ags/media-palette.json".source = repoPackages.mediaPanel.palette;
  xdg.configFile."ags/audio".source =
    config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/audio-control/astal";
  xdg.configFile."ags/audio-palette.json".source = repoPackages.audioPanel.palette;

  systemd.user.services.ags-media = {
    Unit = {
      Description = "AGS/Astal GTK4 media panel";
      X-Restart-Triggers = [ config.xdg.configFile."ags/media-palette.json".source ];
      ConditionEnvironment = [
        "WAYLAND_DISPLAY"
        "XDG_CURRENT_DESKTOP=niri"
      ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${lib.getExe repoPackages.mediaPanel} run";
      Environment = [
        "GDK_BACKEND=wayland"
        "MEDIA_PANEL_CONFIG=${config.xdg.configHome}/ags/media/frontend.js"
        "MEDIA_PANEL_PALETTE=${config.xdg.configHome}/ags/media-palette.json"
        "MEDIA_CONTROL=${lib.getExe repoPackages.mediaControl}"
        "QUICKSHELL=${lib.getExe pkgs.quickshell}"
      ];
      Restart = lib.mkForce "on-failure";
      RestartSec = 3;
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };

  systemd.user.services.audio-control-ui = {
    Unit = {
      Description = "AGS/Astal GTK4 Bluetooth and audio panel";
      X-Restart-Triggers = [ config.xdg.configFile."ags/audio-palette.json".source ];
      ConditionEnvironment = [
        "WAYLAND_DISPLAY"
        "XDG_CURRENT_DESKTOP=niri"
      ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${lib.getExe repoPackages.audioPanel} run";
      Environment = [
        "GDK_BACKEND=wayland"
        "AUDIO_PANEL_CONFIG=${config.xdg.configHome}/ags/audio/frontend.js"
        "AUDIO_PANEL_PALETTE=${config.xdg.configHome}/ags/audio-palette.json"
        "AUDIO_CONTROL_HELPER=${lib.getExe repoPackages.audioControl}"
      ];
      Restart = "on-failure";
      RestartSec = 3;
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };

  systemd.user.services.waybar.Unit = {
    Wants = [ "audio-control-ui.service" ];
    After = [ "audio-control-ui.service" ];
  };
}
