{ pkgs }:
{
  "clock" = {
    format = "󰃭 {:%A %d %B  %H:%M}";
    format-alt = "󰃭 {:%A %Y-%m-%d  %H:%M:%S}";
    tooltip-format = "<tt>{calendar}</tt>";
    calendar = {
      mode = "month";
      mode-switcher = true;
      format = {
        months = "<span color='#ff7edb'><b>{}</b></span>";
        weekdays = "<span color='#7afcff'><b>{}</b></span>";
        days = "<span color='#cbe3e7'>{}</span>";
        today = "<span color='#ff3333'><b><u>{}</u></b></span>";
      };
    };
    on-click = "mode_switch";
    on-scroll = "1";
  };

  "group/system" = {
    orientation = "horizontal";
    drawer = {
      transition-duration = 300;
      transition-left-to-right = true;
    };
    modules = [
      "custom/battery"
      "backlight"
      "pulseaudio"
    ];
  };

  "group/hardware" = {
    orientation = "horizontal";
    drawer = {
      transition-duration = 300;
      transition-left-to-right = true;
    };
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
        Charging|"Not charging")
          icon="󰂄"
          class="charging"
          ;;
        Full)
          icon="󰂄"
          class="full"
          ;;
        *)
          class="discharging"
          if [ "$capacity" -le 10 ]; then
            class="critical"
          elif [ "$capacity" -le 20 ]; then
            class="warning"
          fi
          ;;
      esac

      tooltip="$status | Profile: $profile"

      ${pkgs.jq}/bin/jq -cn --arg text "$icon $capacity%" --arg tooltip "$tooltip" --arg class "$class" \
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

  "backlight" = {
    format = "󰃠 {percent}%";
    tooltip-format = "Backlight: {percent}%";
    on-scroll-up = "${pkgs.brightnessctl}/bin/brightnessctl set 5%+";
    on-scroll-down = "${pkgs.brightnessctl}/bin/brightnessctl set 5%-";
    on-click = "${pkgs.brightnessctl}/bin/brightnessctl set 100%";
  };

  "pulseaudio" = {
    format = "󰕾 {volume}%";
    format-bluetooth = "󰕾 {volume}%";
    format-bluetooth-muted = "󰝟 {volume}%";
    format-muted = "󰝟 {volume}%";
    tooltip-format = "Volume: {volume}%";
    scroll-step = 5;
    on-click-right = "pavucontrol";
    on-click = "pactl set-sink-mute 0 toggle";
  };

  "temperature" = {
    hwmon-path = "";
    thermal-zone = 7;
    critical-threshold = 80;
    interval = 5;
    format = "󰔏 {temperatureC}°C";
    format-critical = "󰔅 {temperatureC}°C";
    tooltip-format = "Sensor: {chip}\n{temperatureC}°C";
  };

  "memory" = {
    interval = 5;
    format = "󰍛 {used:0.1f}G / {total:0.1f}G";
    format-alt = "󰍛 {percentage}%";
    tooltip-format = "RAM: {used:0.1f}G / {total:0.1f}G ({percentage}%)\nSwap: {swapUsed:0.1f}G / {swapTotal:0.1f}G";
  };

  "cpu" = {
    format = "󰻠 {usage}%";
    tooltip = true;
    tooltip-format = "CPU: {usage}%\n{avg_frequency} GHz";
  };

  "disk" = {
    format = "󰋊 {free}";
    format-alt = "󰋊 {percentage_used}% ({free})";
    tooltip = true;
  };

  "network" = {
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

  "power-profiles-daemon" =
    let
      icon = cp: builtins.fromJSON ''"\u${cp}"'';
    in
    {
      format = "{icon}";
      tooltip-format = "Power profile: {profile}\nDriver: {driver}";
      tooltip = true;
      format-icons = {
        default = icon "F0E7";
        performance = icon "F135";
        balanced = icon "F24E";
        power-saver = icon "F06C";
      };
    };
}
