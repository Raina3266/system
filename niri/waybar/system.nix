# Dashboard, Wayle Wi-Fi/media/audio, and Calendar/Tasks modules.
#
# The dashboard button carries the current battery reading and opens Wayle's
# native dashboard, which now also contains notifications and the seven-day
# calendar. Dedicated buttons open Wayle's Wi-Fi, media and audio panels.
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

  # Left-click opens Wayle's Bluetooth and audio panel; the status glyph and
  # tooltip still come from `audio-control status`, which already reports the
  # adapter, the default output and input, and connected devices. Right-click
  # remains the adapter's on/off switch.
  audioModule = monitor: {
    exec = "${packages.audioControl}/bin/audio-control status";
    interval = 5;
    return-type = "json";
    tooltip = true;
    escape = false;
    # `audio-panel` is a second binary in the audio-control crate, so the panel
    # and the Rofi menu make the same decisions about mutually exclusive ALSA
    # card profiles. Running it again toggles the one already up.
    on-click = "${packages.audioControl}/bin/audio-panel ${lib.escapeShellArg monitor}";
    on-click-right = "${packages.audioControl}/bin/audio-control bluetooth-power toggle";
  };

  # The panel this opens is `media-panel`, not one of Wayle's dropdowns: it
  # reads MPRIS off the session bus itself. Running it again toggles the one
  # already up, so the button closes what it opened.
  wayleMediaModule = monitor: {
    format = "{}";
    return-type = "json";
    exec = "${packages.controlCentre}/bin/control-centre media-waybar";
    tooltip = true;
    escape = true;
    "restart-interval" = 2;
    "exec-on-event" = false;
    on-click = "${packages.mediaPanel}/bin/media-panel ${lib.escapeShellArg monitor}";
    # Right-click stops everything. Not `playerctl --all-players pause`: that
    # reads each player's `CanPause` first and skips the ones that answer no,
    # so a browser bridge that cannot reach its tab is never even asked, and
    # the music it is publishing keeps playing.
    on-click-right = "${packages.mediaPanel}/bin/media-panel pause-all";
  };
in
{
  inherit audioModule dashboardModule wayleMediaModule wayleWifiModule;

  homeConfig = {
    home.packages = [
      pkgs.mprisence
      packages.ycal.package
      packages.audioControl
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
    "custom/audio" = audioModule "";
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
