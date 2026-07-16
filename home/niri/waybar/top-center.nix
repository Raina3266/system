{ pkgs }:
{
  "custom/media-prev" = {
    format = "⏮";
    return-type = "json";
    exec = ''echo '{"text":"⏮",}' '';
    exec-if = "${pkgs.playerctl}/bin/playerctl -a status 2>/dev/null | grep -qE '^(Playing|Paused)$'";
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl previous";
  };

  "custom/media" = {
    hide-empty = true;
    format = "{icon} {text}";
    format-icons = {
      "Playing" = "▶";
      "Paused" = "⏸";
      "Stopped" = "⏹";
    };
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-media-poll" ''
      players=$(${pkgs.playerctl}/bin/playerctl -l 2>/dev/null)
      if [ -z "$players" ]; then
          printf '{"text":"","class":"stopped"}'
          exit 0
      fi
      if echo "$players" | grep -qiE 'tauon|kid3'; then
          printf '{"text":"","class":"stopped"}'
          exit 0
      fi
      status=$(${pkgs.playerctl}/bin/playerctl status 2>/dev/null)
      [ -z "$status" ] && status="Stopped"
      artist=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{artist}}' 2>/dev/null)
      title=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{title}}' 2>/dev/null)
      player=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{playerName}}' 2>/dev/null)
      title_short=$(printf '%s' "$title" | cut -c1-40)
      artist_short=$(printf '%s' "$artist" | cut -c1-20)
      if [ -n "$artist_short" ]; then
        text="$artist_short - $title_short"
      else
        text="$title_short"
      fi
      if [ -n "$artist" ]; then
        tooltip="$artist - $title"
      else
        tooltip="$title"
      fi
      [ -n "$player" ] && tooltip="$tooltip\nPlayer: $player\nStatus: $status"
      class=$(echo "$status" | tr '[:upper:]' '[:lower:]')
      ${pkgs.jq}/bin/jq -cn --arg text "$text" --arg class "$class" --arg tooltip "$tooltip" \
        '{text:$text, class:$class, alt:$class, tooltip:$tooltip}'
    '';
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl play-pause";
  };

  "custom/lyrics" = {
    hide-empty-text = true;
    return-type = "json";
    format = "{icon} {0}";
    format-icons = {
      playing = "󰝚";
      paused = "󰝚";
      lyric = "";
      music = "󰝚";
    };
    exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
    exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
    on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
  };

  "custom/media-next" = {
    format = "⏭";
    return-type = "json";
    exec = ''echo '{"text":"⏭",}' '';
    exec-if = "${pkgs.playerctl}/bin/playerctl -a status 2>/dev/null | grep -qE '^(Playing|Paused)$'";
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl next";
  };
}
