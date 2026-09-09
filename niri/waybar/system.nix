# Dashboard, Wayle Wi-Fi/media, and Calendar/Tasks modules.
#
# The dashboard button carries the current battery reading and opens Wayle's
# native dashboard, which now also contains notifications and the seven-day
# calendar. Dedicated buttons open Wayle's Wi-Fi and media panels.
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
    format = "{}";
    return-type = "json";
    interval = 5;
    exec = pkgs.writeShellScript "waybar-dashboard-battery" ''
      bat=""
      for candidate in /sys/class/power_supply/BAT*; do
        if [ -d "$candidate" ]; then
          bat="$candidate"
          break
        fi
      done

      if [ -z "$bat" ]; then
        ${pkgs.jq}/bin/jq -cn \
          --arg text "󰕮" \
          --arg tooltip "Dashboard" \
          '{text:$text, tooltip:$tooltip, class:"clear"}'
        exit 0
      fi

      capacity=$(${pkgs.coreutils}/bin/cat "$bat/capacity" 2>/dev/null)
      status=$(${pkgs.coreutils}/bin/cat "$bat/status" 2>/dev/null)
      profile=$(${pkgs.power-profiles-daemon}/bin/powerprofilesctl get 2>/dev/null)

      icons=("󰁺" "󰁻" "󰁼" "󰁽" "󰁾" "󰁿" "󰂀" "󰂁" "󰂂" "󰁹")
      idx=$((capacity / 10))
      [ "$idx" -gt 9 ] && idx=9
      icon="''${icons[$idx]}"

      case "$status" in
        Charging|"Not charging") icon="󰂄"; class="charging" ;;
        Full) icon="󰂄"; class="full" ;;
        *)
          class="discharging"
          if [ "$capacity" -le 15 ]; then
            class="critical"
          elif [ "$capacity" -le 25 ]; then
            class="warning"
          elif [ "$capacity" -lt 50 ]; then
            class="low"
          fi
          ;;
      esac

      ${pkgs.jq}/bin/jq -cn \
        --arg text "$icon $capacity%" \
        --arg tooltip "$status | Profile: $profile | Click: Dashboard" \
        --arg class "$class" \
        '{text:$text, tooltip:$tooltip, class:$class}'
    '';
    tooltip = true;
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
