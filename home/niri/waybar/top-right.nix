{ pkgs, scripts }:
{
  "custom/audio-sink" = {
    format = "<span size='x-large'>󰕾</span>";
    return-type = "json";
    interval = 5;
    exec = pkgs.writeShellScript "waybar-audio-sink-poll" ''
      icon="<span size='x-large'>󰕾</span>"
      default_sink=$(${pkgs.wireplumber}/bin/wpctl status 2>/dev/null | grep -A1 "Audio" | grep -oP 'id: \K[^,]+' | head -1)
      if [ -z "$default_sink" ]; then
        printf '{"text":"%s","tooltip":"Audio output"}' "$icon"
        exit 0
      fi
      description=$(${pkgs.wireplumber}/bin/wpctl status 2>/dev/null | grep -A5 "$default_sink" | grep -oP 'description: "\K[^"]+' | head -1)
      if [ -z "$description" ]; then
        description="$default_sink"
      fi
      printf '{"text":"%s","tooltip":"Audio: %s"}' "$icon" "$description"
    '';
    on-click = pkgs.writeShellScript "waybar-audio-sink-switch" ''
      ${pkgs.walker}/bin/walker -m menus:audio-sink
    '';
  };

  "tray" = {
    icon-size = 18;
    spacing = 10;
  };

  "custom/cliphist" = {
    format = "<span size='x-large'>󰕛</span>";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-cliphist-poll" ''
      printf '{"text":"<span size='"'"'x-large'"'"'>󰕛</span>","tooltip":"Clipboard history"}'
    '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-cliphist" ''
      ${pkgs.walker}/bin/walker -m clipboard
    '';
  };

  "custom/files" = {
    format = "<span size='x-large'>󰥢</span>";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-files-poll" ''
      printf '{"text":"<span size='"'"'x-large'"'"'>󰥢</span>","tooltip":"Search files"}'
    '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-files" ''
      ${pkgs.walker}/bin/walker -m files
    '';
  };

  "custom/timer" = {
    return-type = "json";
    interval = 1;
    exec = scripts.timerPoll;
    on-click = scripts.timerSet;
    on-click-middle = scripts.timerCancel;
    on-click-right = scripts.timerTogglePause;
    on-scroll-up = scripts.timerScrollUp;
    on-scroll-down = scripts.timerScrollDown;
  };

  "custom/todo" = {
    return-type = "json";
    interval = 2;
    exec = scripts.todoPoll;
    on-click = pkgs.writeShellScript "waybar-todo-open" ''
      ${pkgs.walker}/bin/walker -m todo
    '';
    on-click-right = pkgs.writeShellScript "waybar-todo-add" ''
      ${pkgs.walker}/bin/walker -m todo --search ""
    '';
    on-click-middle = pkgs.writeShellScript "waybar-todo-clear-done" ''
      ${pkgs.walker}/bin/walker -m todo -a clear
    '';
  };

  "custom/bt" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-bt-poll" ''
      on_icon="<span size='x-large'>󰂯</span>"
      off_icon="<span size='x-large'>󰂱</span>"
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        names=$(bluetoothctl devices Connected 2>/dev/null | sed 's/^Device [0-9A-Fa-f:]* //' | tr '\n' ',' | sed 's/,$//')
        if [ -n "$names" ]; then
          printf '{"text":"%s","tooltip":"Connected: %s"}' "$on_icon" "$names"
        else
          printf '{"text":"%s","tooltip":"Bluetooth: On (no devices connected)"}' "$on_icon"
        fi
      else
        printf '{"text":"%s","tooltip":"Bluetooth: Off"}' "$off_icon"
      fi
    '';
    interval = 5;
    on-click = pkgs.writeShellScript "waybar-bt" ''
      ${pkgs.walker}/bin/walker -m bluetooth
    '';
  };

  "custom/wifi" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-wifi-poll" ''
      on_icon="<span size='x-large'>󰤨</span>"
      off_icon="<span size='x-large'>󰤨</span>"
      ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
      if [ -n "$ssid" ]; then
        signal=$(nmcli -t -f active,signal dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
        printf '{"text":"%s","tooltip":"Connected: %s (%s%%)"}' "$on_icon" "$ssid" "$signal"
      else
        printf '{"text":"%s","tooltip":"Wi-Fi: Disconnected"}' "$off_icon"
      fi
    '';
    interval = 5;
    on-click = pkgs.writeShellScript "waybar-wifi" ''
      ${pkgs.walker}/bin/walker -m menus:wifi
    '';
  };

  "custom/powermenu" = {
    format = "<span size='x-large'>󰐥</span>";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-powermenu-poll" ''
      printf '{"text":"<span size='"'"'x-large'"'"'>󰐥</span>","tooltip":"Power menu"}'
    '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-powermenu" ''
      ${pkgs.walker}/bin/walker -m menus:power
    '';
  };
}
