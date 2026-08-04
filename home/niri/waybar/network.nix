# Network module: left-click opens rofi-network-manager (wifi/ethernet),
# right-click opens a dmenu script to control Clash Verge Rev via the
# mihomo external-controller API (switch proxy groups / nodes, toggle mode).
{ pkgs }:
let
  rofi = "${pkgs.rofi}/bin/rofi";
  curl = "${pkgs.curl}/bin/curl";
  jq = "${pkgs.jq}/bin/jq";
  notify = "${pkgs.libnotify}/bin/notify-send";

  # Clash Verge Rev app data dir (runtime config + controller settings).
  # Verge writes the merged runtime config here and reads external-controller
  # + secret from this same file.
  clashConfig = "\${XDG_CONFIG_HOME:-$HOME/.config}/io.github.clash-verge-rev.clash-verge-rev/config.yaml";

  # Resolve controller endpoint + secret from the runtime config.
  # Falls back to the common defaults (127.0.0.1:9090, no secret).
  clashEnv = ''
    api="http://127.0.0.1:9090"
    secret=""
    if [ -f "${clashConfig}" ]; then
      api="$(grep -m1 '^external-controller:' "${clashConfig}" | awk '{print $2}' | tr -d '"' | sed 's#^#http://#')"
      [ -z "$api" ] && api="http://127.0.0.1:9090"
      secret="$(grep -m1 '^secret:' "${clashConfig}" | awk '{print $2}' | tr -d '"')"
    fi
    auth=()
    [ -n "$secret" ] && auth=(-H "Authorization: Bearer $secret")
  '';

  # Pick a proxy group, then a node inside it, and switch via the API.
  clashPick = pkgs.writeShellScript "waybar-clash" ''
    ${clashEnv}
    groups=$(${curl} -s --max-time 5 "''${auth[@]}" "$api/proxies" \
      | ${jq} -r '.proxies | to_entries[] | select(.value.type == "Selector" or .value.type == "URLTest" or .value.type == "Fallback" or .value.type == "LoadBalance") | .key' 2>/dev/null)

    if [ -z "$groups" ]; then
      ${notify} -u critical "Clash Verge" "Could not reach the mihomo controller ($api)"
      exit 1
    fi

    group=$(printf '%s\n' "$groups" | ${rofi} -dmenu -i -p "Proxy group" -theme-str 'window {width: 420px;}')
    [ -z "$group" ] && exit 0

    nodes=$(${curl} -s --max-time 5 "''${auth[@]}" "$api/proxies/$(printf '%s' "$group" | ${jq} -sRr @uri)" \
      | ${jq} -r '.all[]' 2>/dev/null)

    node=$(printf '%s\n' "$nodes" | ${rofi} -dmenu -i -p "Node ($group)" -theme-str 'window {width: 420px;}')
    [ -z "$node" ] && exit 0

    ${curl} -s --max-time 5 -X PUT "''${auth[@]}" \
      -H "Content-Type: application/json" \
      -d "$(${jq} -n --arg n "$node" '{name: $n}')" \
      "$api/proxies/$(printf '%s' "$group" | ${jq} -sRr @uri)" >/dev/null 2>&1 \
      && ${notify} "Clash Verge" "Switched $group → $node"
  '';

  # Toggle the proxy mode (rule / global / direct).
  clashMode = pkgs.writeShellScript "waybar-clash-mode" ''
    ${clashEnv}
    current=$(${curl} -s --max-time 5 "''${auth[@]}" "$api/configs" | ${jq} -r '.mode' 2>/dev/null)
    next="rule"
    case "$current" in
      rule)   next="global" ;;
      global) next="direct" ;;
      direct) next="rule" ;;
    esac
    ${curl} -s --max-time 5 -X PATCH "''${auth[@]}" \
      -H "Content-Type: application/json" \
      -d "$(${jq} -n --arg m "$next" '{mode: $m}')" \
      "$api/configs" >/dev/null 2>&1 \
      && ${notify} "Clash Verge" "Mode: $current → $next"
  '';
in
{
  "custom/wifi" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-wifi-poll" ''
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
    interval = 5;
    # Left: wifi/ethernet manager | Right: Clash Verge proxy picker
    on-click = pkgs.writeShellScript "waybar-wifi" ''
      ${pkgs.rofi-network-manager}/bin/rofi-network-manager
    '';
    on-click-right = clashPick;
    on-click-middle = clashMode;
  };
}
