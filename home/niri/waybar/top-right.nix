{ pkgs, scripts }:
{
  "tray" = {
    icon-size = 18;
    spacing = 10;
  };

  "custom/cliphist" = {
    format = "󰆏";
    return-type = "json";
    exec = ''echo '{"text":"󰆏","tooltip":"Clipboard history"}' '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-cliphist" ''
      ${pkgs.walker}/bin/walker -m clipboard
    '';
  };

  "custom/files" = {
    format = "󰥢";
    return-type = "json";
    exec = ''echo '{"text":"󰥢","tooltip":"Search files"}' '';
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
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        names=$(bluetoothctl devices Connected 2>/dev/null | sed 's/^Device [0-9A-Fa-f:]* //' | tr '\n' ',' | sed 's/,$//')
        if [ -n "$names" ]; then
          printf '{"text":"󰂯","tooltip":"Connected: %s"}' "$names"
        else
          printf '{"text":"󰂯","tooltip":"Bluetooth: On (no devices connected)"}'
        fi
      else
        printf '{"text":"󰂱","tooltip":"Bluetooth: Off"}'
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
      ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
      if [ -n "$ssid" ]; then
        signal=$(nmcli -t -f active,signal dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
        printf '{"text":"󰤨","tooltip":"Connected: %s (%s%%)"}' "$ssid" "$signal"
      else
        printf '{"text":"󰤪","tooltip":"Wi-Fi: Disconnected"}'
      fi
    '';
    interval = 5;
    on-click = pkgs.writeShellScript "waybar-wifi" ''
      ${pkgs.walker}/bin/walker -m menus:wifi
    '';
  };

  "custom/powermenu" = {
    format = "󰐥";
    return-type = "json";
    exec = ''echo '{"text":"󰐥","tooltip":"Power menu"}' '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-powermenu" ''
      ${pkgs.walker}/bin/walker -m menus:power
    '';
  };
}
