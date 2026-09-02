# Battery, media, and Google Calendar/Tasks status modules.
#
# Brightness, volume and the hardware readings are no longer here: they live
# in the SwayNC control center (../swaync), which the bell at the right end of
# the top bar opens. Battery stays in the bar, at its left end, because it is
# the one reading worth seeing without opening anything.
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
    "custom/battery" = {
      return-type = "json";
      interval = 5;
      exec = pkgs.writeShellScript "waybar-battery-poll" ''
        bat=$(ls -d /sys/class/power_supply/BAT* 2>/dev/null | head -1)
        if [ -z "$bat" ]; then
          printf '{"text":"","class":"clear"}'
          exit 0
        fi

        capacity=$(cat "$bat/capacity" 2>/dev/null)
        status=$(cat "$bat/status" 2>/dev/null)
        profile=$(powerprofilesctl get 2>/dev/null)

        icons=("󰁺" "󰁻" "󰁼" "󰁽" "󰁾" "󰁿" "󰂀" "󰂁" "󰂂" "󰁹")
        idx=$(( capacity / 10 ))
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

        tooltip="$status | Profile: $profile"
        ${pkgs.jq}/bin/jq -cn \
          --arg text "$icon $capacity%" \
          --arg tooltip "$tooltip" \
          --arg class "$class" \
          '{text:$text, tooltip:$tooltip, class:$class}'
      '';
      on-click = pkgs.writeShellScript "waybar-battery-cycle" ''
        current=$(powerprofilesctl get 2>/dev/null)
        case "$current" in
          performance) next="balanced" ;;
          balanced) next="power-saver" ;;
          power-saver) next="performance" ;;
          *) next="balanced" ;;
        esac
        powerprofilesctl set "$next" 2>/dev/null
        notify-send "Power Profile" "Set to $next"
      '';
    };

    "custom/media" = {
      hide-empty-text = true;
      format = "{}";
      return-type = "json";
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.mediaControl}/bin/media-control waybar --watch --interval-ms 750";
      tooltip = true;
      escape = true;
      "restart-interval" = 2;
      "exec-on-event" = false;
      on-click = "${packages.mediaControl}/bin/media-control toggle";
      on-click-right = "${packages.mediaControl}/bin/media-control menu";
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
