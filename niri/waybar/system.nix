# Dashboard, media, and Google Calendar/Tasks status modules.
#
# The left dashboard button opens Wayle's native system dashboard. The centre
# media button still carries Wayle's notification count and opens the larger
# calendar/notification control centre.
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
in
{
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
    # Wayle's native dashboard. The Wayle bar itself remains hidden; this D-Bus
    # action toggles the tiny external dashboard host provided by our Wayle patch.
    "custom/dashboard" = {
      format = "󰕮";
      tooltip = true;
      tooltip-format = "Dashboard";
      on-click =
        "${pkgs.systemd}/bin/busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 DashboardToggle";
    };

    # The bar's centre button opens the larger calendar/notification control centre.
    # control-centre draws the whole panel — notifications, calendar, media and
    # system — and the badge is the same program reading Wayle's count over
    # D-Bus, so no shell or jq is in the loop. The track is the mpris module
    # beside this one.
    "custom/media" = {
      format = "{}";
      return-type = "json";
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.controlCentre}/bin/control-centre waybar";
      tooltip = true;
      escape = true;
      "restart-interval" = 2;
      "exec-on-event" = false;
      on-click = "${packages.controlCentre}/bin/control-centre toggle";
      on-click-middle = "${pkgs.wayle}/bin/wayle media play-pause";
    };

    # The track the centre button used to carry. Waybar reads MPRIS itself, so
    # nothing in this repository has to. {dynamic} drops the artist rather
    # than leaving a trailing separator when a player reports none; click,
    # scroll and the rest stay Waybar's own defaults.
    mpris = {
      format = "{status_icon}  {dynamic}";
      format-stopped = "";
      status-icons = {
        playing = "󰐊";
        paused = "󰏤";
      };
      dynamic-order = [
        "title"
        "artist"
      ];
      dynamic-len = 40;
      tooltip-format = "{player} - {status}";
    };

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
