# System, hardware, and media status modules.
{ pkgs, packages }:
let
  drawer = {
    transition-duration = 300;
    transition-left-to-right = true;
  };

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
    home.packages = [ pkgs.mprisence ];
    programs.google-chrome = {
      commandLineArgs = [
        "--disable-features=HardwareMediaKeyHandling,MediaSessionService"
      ];
      nativeMessagingHosts = [ mprisenceNativeHost ];
    };
  };

  modules = {
    "group/system" = {
      orientation = "horizontal";
      inherit drawer;
      modules = [
        "custom/battery"
        "backlight"
        "pulseaudio"
      ];
    };

    "group/hardware" = {
      orientation = "horizontal";
      inherit drawer;
      modules = [
        "temperature"
        "memory"
        "cpu"
        "disk"
        "network"
      ];
    };

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

    backlight = {
      format = "󰃠 {percent}%";
      tooltip-format = "Backlight: {percent}%";
      on-scroll-up = "${pkgs.brightnessctl}/bin/brightnessctl set 5%+";
      on-scroll-down = "${pkgs.brightnessctl}/bin/brightnessctl set 5%-";
      on-click = "${pkgs.brightnessctl}/bin/brightnessctl set 100%";
    };

    pulseaudio = {
      format = "󰕾 {volume}%";
      format-bluetooth = "󰕾 {volume}%";
      format-bluetooth-muted = "󰝟 {volume}%";
      format-muted = "󰝟 {volume}%";
      tooltip-format = "Volume: {volume}%";
      scroll-step = 5;
      on-click-right = "pavucontrol";
      on-click = "pactl set-sink-mute 0 toggle";
    };

    temperature = {
      thermal-zone = 8;
      warning-threshold = 55;
      critical-threshold = 80;
      interval = 5;
      format = "󰄏 {temperatureC}°C";
      format-critical = "󰄅 {temperatureC}°C";
      tooltip-format = "CPU package: {temperatureC}°C";
    };

    memory = {
      interval = 5;
      format = "󰍛 {used:0.1f}G / {total:0.1f}G";
      format-alt = "󰍛 {percentage}%";
      tooltip-format = "RAM: {used:0.1f}G / {total:0.1f}G ({percentage}%)\nSwap: {swapUsed:0.1f}G / {swapTotal:0.1f}G";
    };

    cpu = {
      format = "󰻠 {usage}%";
      tooltip = true;
      tooltip-format = "CPU: {usage}%\n{avg_frequency} GHz";
    };

    disk = {
      format = "󰋊 {free}";
      format-alt = "󰋊 {percentage_used}% ({free})";
      tooltip = true;
    };

    network = {
      format = "󰖩  {bandwidthDownBytes}";
      format-disconnected = "󰖪 Disconnected";
      format-alt = "󰖩  {bandwidthUpBytes} |  {bandwidthDownBytes}";
      format-wifi = "󰖩  {bandwidthDownBytes}";
      format-ethernet = "󰈀  {bandwidthDownBytes}";
      tooltip-format-wifi = "󰖩 {essid} ({signalStrength}%)\n {ipaddr}\n {bandwidthUpBytes} /  {bandwidthDownBytes}";
      tooltip-format-ethernet = "󰈀 {ifname}: {ipaddr}/{cidr}\n {bandwidthUpBytes} /  {bandwidthDownBytes}";
      tooltip-format-disconnected = "󰖪 Disconnected";
      on-click-right = "nm-connection-editor";
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
  };
}
