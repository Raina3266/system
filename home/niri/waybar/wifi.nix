# Wi-Fi / VPN module: left-click opens rofi-network-manager (wifi/ethernet),
# right-click opens rofi-vpn (toggle existing VPN connections).
{ pkgs }:
let
  wifiPoll = pkgs.writeShellScript "waybar-wifi-poll" ''
    on_icon="<span size='large'>󰤨 </span>"
    off_icon="<span size='large'>󰤮 </span>"
    state=$(nmcli -t -f WIFI g 2>/dev/null)
    if [ "$state" = "enabled" ]; then
      ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | awk -F: '$1=="yes"{print $2; exit}')
      if [ -n "$ssid" ]; then
        printf '{"text":"%s","tooltip":"Wi-Fi: %s"}' "$on_icon" "$ssid"
      else
        printf '{"text":"%s","tooltip":"Wi-Fi: On (not connected)"}' "$on_icon"
      fi
    else
      printf '{"text":"%s","tooltip":"Wi-Fi: Off"}' "$off_icon"
    fi
  '';

  wifiOpen = pkgs.writeShellScript "waybar-wifi" ''
    ${pkgs.rofi-network-manager}/bin/rofi-network-manager
  '';

  vpnOpen = pkgs.writeShellScript "waybar-vpn" ''
    ${pkgs.rofi-vpn}/bin/rofi-vpn
  '';
in
{
  "custom/wifi" = {
    format = "{}";
    return-type = "json";
    exec = wifiPoll;
    interval = 5;
    on-click = wifiOpen;
    on-click-right = vpnOpen;
  };
}
