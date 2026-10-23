# Dashboard, network/media/audio, and Calendar/Tasks modules.
#
# Media, audio, dashboard, and network use AGS/Astal GTK4.
{
  lib,
  pkgs,
  packages,
}:
let
  mprisenceNativeHost =
    pkgs.writeTextDir "etc/chromium/native-messaging-hosts/mprisence.web.bridge.json"
      (
        builtins.toJSON {
          name = "mprisence.web.bridge";
          description = "Publish each browser media tab as an MPRIS player";
          path = "${pkgs.mprisence}/bin/mprisence";
          type = "stdio";
          allowed_origins = [
            "chrome-extension://pnkkjbdopihogobhhjbgapbpfccinjjo/"
            "chrome-extension://pphdmbejbipjlocngoefnmjoijcbdejf/"
          ];
        }
      );

  dashboardModule = monitor: {
    format = "{}";
    return-type = "json";
    interval = 5;
    exec = "${lib.getExe packages.dashboardPanel} status";
    tooltip = true;
    on-click = "${lib.getExe packages.dashboardPanel} toggle ${lib.escapeShellArg monitor}";
  };

  networkModule = monitor: {
    exec = "${lib.getExe packages.networkPanel} status";
    interval = 5;
    return-type = "json";
    tooltip = true;
    escape = false;
    on-click = "${lib.getExe packages.networkPanel} toggle ${lib.escapeShellArg monitor}";
  };

  # The resident AGS/Astal audio panel owns the badge and Bluetooth/Audio popup.
  audioModule = monitor: {
    exec = "${lib.getExe packages.audioPanel} status";
    interval = 5;
    return-type = "json";
    tooltip = true;
    escape = false;
    on-click = "${lib.getExe packages.audioPanel} toggle ${lib.escapeShellArg monitor}";
    on-click-middle = "${lib.getExe packages.audioPanel} bluetoothPower";
  };

  # The resident AGS/Astal media panel owns source selection and playback.
  # Its launcher keeps AGS instance/request details out of Waybar.
  mediaModule = monitor: {
    format = "{}";
    return-type = "json";
    exec = "${lib.getExe packages.mediaPanel} status";
    tooltip = true;
    escape = true;
    interval = 1;
    "exec-on-event" = false;
    on-click = "${lib.getExe packages.mediaPanel} toggle ${lib.escapeShellArg monitor}";
    on-click-middle = "${lib.getExe packages.mediaPanel} togglePauseAll";
    on-click-right = "${lib.getExe packages.mediaPanel} cycleMedia";
  };
in
{
  inherit
    audioModule
    dashboardModule
    networkModule
    mediaModule
    ;

  homeConfig = {
    home.packages = [
      pkgs.mprisence
      packages.ycal.package
      packages.audioControl
      packages.networkManager
    ];
    programs.google-chrome = {
      commandLineArgs = [
        "--disable-features=HardwareMediaKeyHandling,MediaSessionService"
      ];
      nativeMessagingHosts = [ mprisenceNativeHost ];
    };
    systemd.user.services.waybar-ycal = {
      Unit = {
        Description = "waybar-ycal: Google Calendar and Tasks popup";
        ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${packages.ycal.package}/bin/waybar-ycal-popup";
        Restart = lib.mkForce "on-failure";
        RestartSec = 3;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };

  modules = {
    # Generic fallback for callers that do not create a bar per output. The
    # actual Niri bars override these with their connector name so each UI opens
    # each panel on the monitor whose button was clicked.
    "custom/audio" = audioModule "";
    "custom/dashboard" = dashboardModule "";
    "custom/network" = networkModule "";
    "custom/media" = mediaModule "";

    "custom/lyrics" = {
      hide-empty-text = true;
      return-type = "json";
      format = "󰝚  {text}";
      exec-if = "pgrep -x tauon >/dev/null || pgrep -x elisa >/dev/null || pgrep -x kid3 >/dev/null";
      exec = "${packages.withParentDeath}/bin/with-parent-death ${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
      on-click = "${packages.withParentDeath}/bin/with-parent-death ${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
    };

    "custom/ycal" = {
      return-type = "json";
      interval = 60;
      exec = "${packages.ycal.bar}/bin/waybar-ycal-bar";
      on-click = "${packages.ycal.toggle}/bin/waybar-ycal-toggle";
    };
  };
}
