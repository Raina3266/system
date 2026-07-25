{ pkgs }:
let
  walker = "${pkgs.walker}/bin/walker";
  elephant = "${pkgs.elephant}/bin/elephant";
  jq = "${pkgs.jq}/bin/jq";

  todoFile = "\${XDG_CACHE_HOME:-$HOME/.cache}/elephant/todo.csv";

  # Todo module: shows current task + count, tooltip lists pending tasks
  todoPoll = pkgs.writeShellScript "waybar-todo-poll" ''
    icon="<span size='x-large'>󰄲 </span>"
    if [ ! -f "${todoFile}" ]; then
      printf '{"text":"%s","tooltip":"Todo","class":"clear"}' "$icon"
      exit 0
    fi

    # "active" is used as a pin: elephant's todo provider has no pin
    # action, but `active` is a Return-toggled state that it already
    # sorts to the top of the list, so it serves the same purpose.
    pending=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent" || $3=="active") {c++} END{print c+0}' "${todoFile}")
    total=$(awk -F';' 'NR>1 {c++} END{print c+0}' "${todoFile}")

    if [ "$pending" -eq 0 ] 2>/dev/null; then
      text="$icon <span size='medium'>Add a task!</span>"
      ${jq} -cn --arg text "$text" --arg tooltip "No pending tasks 🎉" --arg class "clear" \
        '{text:$text, tooltip:$tooltip, class:$class}'
      exit 0
    fi

    actionable=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent" || $3=="active") && $6!="" {
      cmd="date -d \"" $6 "\" +%s 2>/dev/null"
      cmd | getline ts; close(cmd)
      cmd="date -d \"today 23:59:59\" +%s"
      cmd | getline eod; close(cmd)
      if (ts!="" && ts+0 <= eod+0) c++
    } END{print c+0}' "${todoFile}")

    # Get the task to display: pinned (active) first, then urgent, then
    # by deadline ($6 = scheduled), then file order.
    current=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent" || $3=="active") {
      pinned = ($3=="active") ? 0 : 1
      urgent = ($3=="urgent") ? 0 : 1
      ts = 9999999999
      if ($6!="") {
        cmd="date -d \"" $6 "\" +%s 2>/dev/null"
        cmd | getline t; close(cmd)
        if (t!="") ts = t+0
      }
      key = pinned "" urgent "" sprintf("%010d", ts)
      if (!found || key < best) { found=1; best=key; text=$2; st=$3 }
    } END{print (st=="active" ? "" : "") text}' "${todoFile}")

    # Truncate to 40 chars
    current_short=$(printf '%s' "$current" | cut -c1-30)
    if [ "''${#current}" -gt 40 ]; then
      current_short="$current_short…"
    fi

    if [ "$actionable" -gt 0 ] 2>/dev/null; then
      class="urgent"
    else
      class="pending"
    fi

    text="$icon <span size='medium'>$current_short</span>  <span size='small'>($total)</span>"

    # Tooltip: pending task list, pinned first
    list=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent" || $3=="active") {
      if ($3=="active") print "0\t  " $2
      else print "1\t  " $2
    }' "${todoFile}" | sort -k1,1 | cut -f2- | head -10)

    tooltip="$pending pending · $actionable due today/overdue"$'\n\n'"$list"

    ${jq} -cn --arg text "$text" --arg tooltip "$tooltip" --arg class "$class" \
      '{text:$text, tooltip:$tooltip, class:$class}'
  '';

  # Media control buttons
  mediaButton = glyph: cmd: {
    format = "<span size='x-large'>${glyph}</span>";
    return-type = "json";
    exec = ''printf '{"text":"${glyph}"}' '';
    exec-if = "${pkgs.playerctl}/bin/playerctl -a status 2>/dev/null | grep -qE '^(Playing|Paused)$'";
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl ${cmd}";
  };
in
{
  "custom/media-prev" = mediaButton "⏮" "previous";

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
      all_players=$(${pkgs.playerctl}/bin/playerctl -l 2>/dev/null)
      if [ -z "$all_players" ]; then
          printf '{"text":"","class":"stopped"}'
          exit 0
      fi
      # Pick the first active player that isn't tauon or kid3
      player_name=$(echo "$all_players" | grep -ivE 'tauon|kid3' | head -1)
      if [ -z "$player_name" ]; then
          printf '{"text":"","class":"stopped"}'
          exit 0
      fi
      status=$(${pkgs.playerctl}/bin/playerctl -p "$player_name" status 2>/dev/null)
      [ -z "$status" ] && status="Stopped"
      artist=$(${pkgs.playerctl}/bin/playerctl -p "$player_name" metadata --format '{{artist}}' 2>/dev/null)
      title=$(${pkgs.playerctl}/bin/playerctl -p "$player_name" metadata --format '{{title}}' 2>/dev/null)
      player=$(${pkgs.playerctl}/bin/playerctl -p "$player_name" metadata --format '{{playerName}}' 2>/dev/null)
      # Some players (e.g. VLC) leave title empty for video files — fall back to filename
      [ -z "$title" ] && title=$(${pkgs.playerctl}/bin/playerctl -p "$player_name" metadata xesam:url 2>/dev/null)
      case "$title" in
        file://*|/*)
          path="''${title#file://}"
          title=$(basename -- "$(printf '%b' "''${path//%/\\x}")")
          ;;
      esac
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
      [ -n "$player" ] && tooltip="$tooltip\\nPlayer: $player"
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
      playing = "󰝚 ";
      paused = "󰝚 ";
      lyric = "";
      music = "󰝚 ";
    };
    exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
    exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
    on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
  };

  "custom/media-next" = mediaButton "⏭" "next";

  "custom/todo" = {
    return-type = "json";
    interval = 2;
    exec = todoPoll;
    # left=search, right=create
    on-click = pkgs.writeShellScript "waybar-todo-search" ''
      ${elephant} activate "todo;;search;;" || true
      exec ${walker} -t cyberpunk-center -m todo
    '';
    on-click-right = pkgs.writeShellScript "waybar-todo-create" ''
      ${elephant} activate "todo;;create;;" || true
      exec ${walker} -t cyberpunk-center -m todo
    '';
  };
}
