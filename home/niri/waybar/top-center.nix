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

    pending=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") {c++} END{print c+0}' "${todoFile}")
    total=$(awk -F';' 'NR>1 {c++} END{print c+0}' "${todoFile}")

    if [ "$pending" -eq 0 ] 2>/dev/null; then
      text="$icon <span size='medium'>Add a task!</span>"
      ${jq} -cn --arg text "$text" --arg tooltip "No pending tasks 🎉" --arg class "clear" \
        '{text:$text, tooltip:$tooltip, class:$class}'
      exit 0
    fi

    actionable=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") && $6!="" {
      cmd="date -d \"" $6 "\" +%s 2>/dev/null"
      cmd | getline ts; close(cmd)
      cmd="date -d \"today 23:59:59\" +%s"
      cmd | getline eod; close(cmd)
      if (ts!="" && ts+0 <= eod+0) c++
    } END{print c+0}' "${todoFile}")

    # Get top-priority task (urgent first, then by schedule, then file order)
    current=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") {
      urgent = ($3=="urgent") ? 0 : 1
      ts = 9999999999
      if ($6!="") {
        cmd="date -d \"" $6 "\" +%s 2>/dev/null"
        cmd | getline t; close(cmd)
        if (t!="") ts = t+0
      }
      key = urgent * 10000000000 + ts
      if (!found || key < best) { found=1; best=key; text=$2 }
    } END{print text}' "${todoFile}")

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

    # Tooltip: pending task list
    list=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") {
      print "⬜  " $2
    }' "${todoFile}" | head -10)

    tooltip="$pending pending · $actionable due today/overdue"$'\n\n'"$list"

    ${jq} -cn --arg text "$text" --arg tooltip "$tooltip" --arg class "$class" \
      '{text:$text, tooltip:$tooltip, class:$class}'
  '';

  # Media control buttons
  mediaButton = glyph: cmd: {
    format = "<span size='x-large'>${glyph}</span>";
    return-type = "json";
    exec = ''echo '{"text":"${glyph}",}' '';
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
    # left=search, right=create, middle=mark done
    on-click = pkgs.writeShellScript "waybar-todo-search" ''
      ${elephant} activate "todo;;search;;" || true
      exec ${walker} -t cyberpunk-center -m todo
    '';
    on-click-right = pkgs.writeShellScript "waybar-todo-create" ''
      ${elephant} activate "todo;;create;;" || true
      exec ${walker} -t cyberpunk-center -m todo
    '';
    on-click-middle = pkgs.writeShellScript "waybar-todo-done" ''
      ${elephant} activate "todo;;done;;" || true
    '';
  };
}
