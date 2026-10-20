# Dashboard, network/media/audio, and Calendar/Tasks modules.
#
# Dashboard, network, media, and audio use native Quickshell panels.
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
      "${lib.getExe pkgs.quickshell} -c media ipc call dashboard toggle ${lib.escapeShellArg monitor}";
  };

  networkModule = monitor: {
    exec = "${lib.getExe pkgs.quickshell} ipc --config media call network status";
    interval = 5;
    return-type = "json";
    tooltip = true;
    escape = false;
    on-click =
      "${lib.getExe pkgs.quickshell} ipc --config media call network toggle ${lib.escapeShellArg monitor}";
  };

  # Quickshell owns Pair/Output/Input; the helper handles physical profiles.
  audioModule = monitor: {
    exec = "${packages.audioControl}/bin/audio-control status";
    interval = 5;
    return-type = "json";
    tooltip = true;
    escape = false;
    on-click =
      "${pkgs.quickshell}/bin/quickshell ipc --config audio-control call panel toggle ${lib.escapeShellArg monitor}";
    on-click-middle = "${pkgs.quickshell}/bin/quickshell ipc --config audio-control call panel bluetoothPower";
  };

  # The old GTK media-panel has been replaced: the popup is now the
  # resident Quickshell media panel (themes/quickshell/), toggled over IPC. It lists
  # every playing or paused source, seeks, and routes each stream through
  # audio-control's profile-aware move.
  mediaModule = monitor: {
    format = "{}";
    return-type = "json";
    exec = "${lib.getExe pkgs.quickshell} -c media ipc call media status";
    tooltip = true;
    escape = true;
    interval = 1;
    "exec-on-event" = false;
    on-click = "${lib.getExe pkgs.quickshell} -c media ipc call media toggle ${lib.escapeShellArg monitor}";
    on-click-middle = "${lib.getExe pkgs.quickshell} -c media ipc call media togglePauseAll";
    on-click-right = "${lib.getExe pkgs.quickshell} -c media ipc call media cycleMedia";
  };
in
{
  inherit audioModule dashboardModule networkModule mediaModule;

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
