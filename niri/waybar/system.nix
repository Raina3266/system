# Dashboard, Wayle Wi-Fi/media, notifications, and Calendar/Tasks modules.
#
# Dedicated Waybar buttons open Wayle's native dashboard, Wi-Fi manager, and
# media panel. The notification badge opens the calendar/notification panel.
{ lib, pkgs, packages }:
let
  mprisenceNativeHost =
    pkgs.writeTextDir "etc/chromium/native-messaging-hosts/mprisence.web.bridge.json"
      (builtins.toJSON {
        name = "mprisence.web.bridge";
        description = "Publish each browser media tab as an MPRIS player";
        path = "${pkgs.mprisence}/bin/mprisence";
        type = "stdio";
        allowed_origins = [
          "chrome-extension://pnkkjbdopihogobhhjbgapbpfccinjjo/"
          "chrome-extension://pphdmbejbipjlocngoefnmjoijcbdejf/"
        ];
      });

  dashboardModule = monitor: {
    format = "󰕮";
    tooltip = true;
    tooltip-format = "Dashboard";
    on-click =
      "${pkgs.systemd}/bin/busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 DropdownToggle ss dashboard ${lib.escapeShellArg monitor}";
  };

  wayleWifiModule = monitor: {
    format = "󰤨";
    tooltip = true;
    tooltip-format = "Wayle Wi-Fi";
    on-click =
      "${pkgs.systemd}/bin/busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 DropdownToggle ss network ${lib.escapeShellArg monitor}";
  };

  wayleMediaModule = monitor: {
    format = "{}";
    return-type = "json";
    exec = "${packages.controlCentre}/bin/control-centre media-waybar";
    tooltip = true;
    escape = true;
    "restart-interval" = 2;
    "exec-on-event" = false;
    on-click =
      "${pkgs.systemd}/bin/busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 DropdownToggle ss media ${lib.escapeShellArg monitor}";
    on-click-middle = "${pkgs.wayle}/bin/wayle media play-pause";
  };
in
{
  inherit dashboardModule wayleMediaModule wayleWifiModule;

  homeConfig = {
    home.packages = [
      pkgs.mprisence
      packages.ycal.package
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
    # actual Niri bars override these with their connector name so Wayle opens
    # each panel on the monitor whose button was clicked.
    "custom/dashboard" = dashboardModule "";
    "custom/wayle-wifi" = wayleWifiModule "";
    "custom/wayle-media" = wayleMediaModule "";

    # The notification badge and calendar share the remaining control-centre.
    # The badge is the same program reading Wayle's count over D-Bus, so no
    # shell or jq is in the loop.
    "custom/notifications" = {
      format = "{}";
      return-type = "json";
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.controlCentre}/bin/control-centre waybar";
      tooltip = true;
      escape = true;
      "restart-interval" = 2;
      "exec-on-event" = false;
      on-click = "${packages.controlCentre}/bin/control-centre toggle";
    };

    # The media button above reads Wayle's own D-Bus service, so it and the
    # popup select and deduplicate the same native player list.
    "custom/lyrics" = {
      hide-empty-text = true;
      return-type = "json";
      format = "󰝚  {text}";
      exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
      exec = "${packages.withParentDeath}/bin/with-parent-death ${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
      on-click = "${packages.withParentDeath}/bin/with-parent-death ${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
    };

    "custom/ycal" = {
      return-type = "json";
      interval = 60;
      exec = packages.ycal.barExec;
      on-click = packages.ycal.toggle;
    };
  };
}
