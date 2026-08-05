# Network module (Wi-Fi indicator). A self-contained rofi front-end for
# NetworkManager, so nm-connection-editor is never needed.
#
# The list only ever holds networks/devices; commands live in a clickable
# button toolbar along the bottom (see `navmenu`), so the two never mix.
#
#   left   -> Wi-Fi: scan list + toolbar (rescan, hidden, saved, status).
#             Pick a network for connect / disconnect /
#             share password + QR / edit / forget. If the radio is off,
#             opening this menu turns it back on and shows the scan list.
#   middle -> toggle the Wi-Fi radio (the only way to turn it off)
#   right  -> Ethernet: device list + toolbar (status)
#
# VPN/proxy lives in the Clash Verge Rev app, not here.
{ pkgs }:
let
  rofi = "${pkgs.rofi}/bin/rofi";
  nmcli = "${pkgs.networkmanager}/bin/nmcli";
  notify = "${pkgs.libnotify}/bin/notify-send";
  qrencode = "${pkgs.qrencode}/bin/qrencode";
  wlcopy = "${pkgs.wl-clipboard}/bin/wl-copy";

  # Shared dmenu invocation; callers append `-p "Prompt"` and any extra flags.
  # Later -theme-str flags win, so callers can override the window geometry.
  menu = ''${rofi} -dmenu -i -theme "$HOME/.config/rofi/rofi-single.rasi" '';

  # icon_for <signal 0-100> <secured yes|no> -- Wi-Fi strength glyph.
  iconFor = ''
    icon_for() {
      local t=$(( (''${1:-0} + 24) / 25 ))
      local bars_open=("󰤯" "󰤟" "󰤢" "󰤥" "󰤨")
      local bars_lock=("󰤯" "󰤡" "󰤤" "󰤧" "󰤪")
      if [ "$2" = yes ]; then printf '%s' "''${bars_lock[t]}"; else printf '%s' "''${bars_open[t]}"; fi
    }
  '';

  helpers = ''
    # report "message" cmd... -- notify on success, loudly on failure.
    report() {
      msg="$1"; shift
      if "$@" >/dev/null; then
        ${notify} "Network" "$msg"
      else
        ${notify} -u critical "Network" "Failed: $msg"
      fi
    }

    # choose "Prompt" "key|Label"... -- shows the labels, echoes the chosen key.
    # Selecting by index keeps matching exact, so decorated labels never collide.
    # Empty arguments are dropped, so callers can splice in a conditional entry
    # (e.g. "$(back_entry)") without it turning into a blank row when absent.
    choose() {
      local prompt="$1" i e
      local -a opts=()
      shift
      for e in "$@"; do [ -n "$e" ] && opts+=("$e"); done
      [ "''${#opts[@]}" -gt 0 ] || return 0
      i=$(printf '%s\n' "''${opts[@]}" | cut -d'|' -f2- \
        | ${menu} -p "$prompt" -format i -no-custom)
      [ -n "$i" ] || return 0
      printf '%s' "''${opts[i]}" | cut -d'|' -f1
    }

    # Back navigation. The top-level menus export NET_ROOT="$0"; every submenu
    # below them offers a Back affordance that returns there, at any depth. Using
    # an exported variable rather than threading a script path through argv keeps
    # nested calls simple and avoids the submenus having to refer to each other
    # by name, which Nix cannot evaluate (mutual recursion between derivations).
    #
    # nav_back -- add a Back button to nav_labels, if there is a menu to go to.
    nav_back() { [ -n "''${NET_ROOT-}" ] && nav_labels+=("󰌍  Back"); return 0; }
    # back_entry -- echo a `choose` entry for Back, or nothing at top level.
    back_entry() { [ -n "''${NET_ROOT-}" ] && printf 'back|󰌍  Back'; return 0; }
    # go_back -- return to the root menu, or just leave if we are already there.
    # Replaces the current process, so the menu stack never grows.
    go_back() { [ -n "''${NET_ROOT-}" ] && exec "$NET_ROOT"; exit 0; }

    # ask "Prompt" [prefill] -- free text, prefilled value is editable in place.
    ask() { ${menu} -p "$1" -filter "''${2-}" </dev/null; }
    # askpw "Prompt" -- hidden entry.
    askpw() { ${menu} -p "$1" -password </dev/null; }
    # pick "Prompt" choice... -- one of a fixed set.
    pick() { printf '%s\n' "''${@:2}" | ${menu} -p "$1" -no-custom; }
    # info "Prompt" -- read-only list, fed on stdin.
    info() { ${menu} -p "$1" -no-custom -theme-str 'entry {enabled: false;}' >/dev/null; }

    # navmenu "Prompt" -- list on stdin, plus a clickable button bar built from
    # the caller's nav_labels array. Keeps commands out of the list itself: rows
    # are only ever real networks/devices, and the bar grows to fit the array.
    # Echoes the picked row index; exit status 0 = row, 10+n = nav_labels[n].
    #
    # The bar wraps at nav_per_row buttons per line. rofi has no flow/wrap
    # layout, so each line is an explicit box (navrow1..navrowN) holding a slice
    # of the buttons, and the vertical navbar stacks those boxes. Per
    # rofi-theme(5) ("Advanced layout"), any widget name not starting with a
    # known special prefix is built as a box that can pack other widgets, which
    # is what makes the runtime-generated navrow* widgets valid row containers.
    #
    # `button` is a documented first-class widget (a textbox with a clickable
    # `action`), so this whole bar is the intended mechanism, not a workaround.
    nav_labels=()
    nav_per_row=3
    navmenu() {
      local i=0 r=0 defs="" rows="" rowdefs="" cur="" l
      if [ "''${#nav_labels[@]}" -eq 0 ]; then
        ${menu} -p "$1" -format i -no-custom
        return
      fi
      for l in "''${nav_labels[@]}"; do
        i=$((i + 1))
        # The name must start with "button": rofi picks a widget's type from its
        # name prefix, and only real buttons honour content/action. Anything
        # else parses fine but is built as a box, silently ignoring both. The
        # theme pre-styles button1..8, so these names must stay in that range.
        #
        # Each button completes kb-custom-$i, which dmenu reports as exit 9+$i.
        # One property per line, and `content` last in its own block: rofi's
        # -theme-str lexer reads a quoted value greedily to the *last* quote on
        # the line, so "$l"; action: "kb-custom-$i" would all land in `content`
        # (the label then renders as the raw css) and `action` would go unset,
        # making the button unclickable. A newline terminates the string, so
        # keeping each string property alone on its line parses correctly.
        defs="$defs
          button$i {action: \"kb-custom-$i\";}
          button$i {content: \"$l\";}"
        # Close the current line every nav_per_row buttons.
        cur="''${cur:+$cur, }button$i"
        if [ $((i % nav_per_row)) -eq 0 ]; then
          r=$((r + 1))
          rows="''${rows:+$rows, }navrow$r"
          rowdefs="$rowdefs
          navrow$r {children: [ $cur ];}"
          cur=""
        fi
      done
      # Flush the trailing partial line.
      if [ -n "$cur" ]; then
        r=$((r + 1))
        rows="''${rows:+$rows, }navrow$r"
        rowdefs="$rowdefs
          navrow$r {children: [ $cur ];}"
      fi
      # Only structure is injected here -- which navrow holds which buttons, and
      # each button's content/action. All appearance (navbar orientation and
      # padding, navrow layout, the button pills) lives in rofi-single.rasi,
      # which pre-declares button1..8 and navrow1..3 by name. That is necessary
      # because rofi resolves properties from a widget's *own* name/path rather
      # than by walking ancestors: it looks up `#button1`, `#button2`, ... and
      # never consults `navbar` or a generic `button`. (Confirmed with
      # `G_MESSAGES_DEBUG=Theme rofi ...`, which logs `Theme entry: #button1
      # property border unset.`) Since the names are predictable, the theme can
      # declare them up front and the script needs no styling at all.
      #
      # -no-custom would return immediately when the list is empty (radio off, or
      # nothing in range), making the bar unreachable, so it is left off.
      ${menu} -p "$1" -format i \
        -theme-str "navbar {children: [ $rows ];}
          $rowdefs
          $defs"
    }
  '';

  # Read-only overview of every wired/wireless device, replacing the "details"
  # pane of nm-connection-editor.
  statusView = pkgs.writeShellScript "waybar-net-status" ''
    ${helpers}
    {
      printf 'Wi-Fi radio: %s\n' "$(${nmcli} radio wifi)"
      while IFS=: read -r dev type state conn; do
        case "$type" in wifi | ethernet) ;; *) continue ;; esac
        printf '%s  %s%s\n' "$dev" "$state" "''${conn:+ — $conn}"
        [ "$state" = connected ] || continue
        printf '    IPv4  %s    gateway %s\n' \
          "$(${nmcli} -g IP4.ADDRESS device show "$dev" | tr '\n' ' ')" \
          "$(${nmcli} -g IP4.GATEWAY device show "$dev")"
        printf '    DNS   %s\n' "$(${nmcli} -g IP4.DNS device show "$dev" | tr '\n' ' ')"
        [ "$type" = wifi ] && printf '    Link  %s\n' \
          "$(${nmcli} -g SSID,SIGNAL,RATE device wifi list ifname "$dev" \
            | sed 's/\\:/:/g' | awk -F: -v s="$conn" '$1==s{print $2"%  "$3; exit}')"
      done < <(${nmcli} -g DEVICE,TYPE,STATE,CONNECTION device status)
    } | info "Status"
    # Dismissing the read-only view returns to whichever menu opened it.
    go_back
  '';

  # Reveal / copy / QR-encode the PSK of a saved Wi-Fi profile.
  # Usage: sharePass <connection-name>
  sharePass = pkgs.writeShellScript "waybar-wifi-share" ''
    ${helpers}
    conn="$1"
    [ -n "$conn" ] || exit 0

    # -s is required for nmcli to emit secrets at all.
    psk=$(${nmcli} -s -g 802-11-wireless-security.psk connection show id "$conn" 2>/dev/null)
    [ -n "$psk" ] && kind=WPA || kind=nopass
    # A profile name may differ from the SSID it actually joins.
    ssid=$(${nmcli} -g 802-11-wireless.ssid connection show id "$conn" 2>/dev/null)
    ssid="''${ssid:-$conn}"

    # The WIFI: URI treats \ ; , : as separators, so they must be escaped.
    esc() { printf '%s' "$1" | sed 's/[\\;,:"]/\\&/g'; }

    case "$(choose "$conn" \
      "pw|󰌾  ''${psk:-(open network)}" \
      "copy|  Copy password" \
      "qr|󰀲  Show QR code" \
      "$(back_entry)")" in
      back) go_back ;;
      copy)
        [ -n "$psk" ] && printf '%s' "$psk" | ${wlcopy} && ${notify} "Wi-Fi" "Password copied"
        ;;
      qr)
        png="''${XDG_RUNTIME_DIR:-/tmp}/wifi-qr.png"
        ${qrencode} -t PNG -o "$png" -l H -s 10 -m 2 \
          "WIFI:T:$kind;S:$(esc "$ssid");P:$(esc "$psk");;" || exit 1
        printf '\n' | ${menu} -p "" -theme-str \
          'mainbox {children: [ listview ];} listview {lines: 0;}' >/dev/null
        rm -f "$png"
        ;;
    esac
  '';

  # In-rofi replacement for nm-connection-editor, scoped to the properties
  # worth changing by hand. Loops so several edits can be made in one go.
  # Usage: editConn <connection-name>
  editConn = pkgs.writeShellScript "waybar-conn-edit" ''
    ${helpers}
    conn="$1"
    [ -n "$conn" ] || exit 0

    get() { ${nmcli} -g "$1" connection show "$conn" 2>/dev/null; }
    apply() {
      report "$conn: $1 = ''${2:-(cleared)}" ${nmcli} connection modify "$conn" "$1" "$2"
    }
    [ "$(get connection.type)" = 802-11-wireless ] && wifi=yes || wifi=no

    while :; do
      entries=(
        "auto|󰸋  Autoconnect: $(get connection.autoconnect)"
        "prio|󰒡  Autoconnect priority: $(get connection.autoconnect-priority)"
      )
      [ "$wifi" = yes ] && entries+=(
        "psk|󰌾  Change Wi-Fi password"
        "share|  Show password / QR"
      )
      entries+=(
        "v4|󰩠  IPv4 method: $(get ipv4.method)"
        "addr|󰩟  Address: $(get ipv4.addresses)"
        "gw|󰱩  Gateway: $(get ipv4.gateway)"
        "dns|󰗧  DNS: $(get ipv4.dns)"
        "v6|󰩠  IPv6 method: $(get ipv6.method)"
        "rename|󰱕  Rename"
        "delete|  Delete"
      )
      # Back returns to the menu that opened this editor; Done just closes.
      entries+=("$(back_entry)" "done|󰌑  Done")

      case "$(choose "Edit $conn" "''${entries[@]}")" in
        back) go_back ;;
        auto)
          v=$(pick Autoconnect yes no)
          [ -n "$v" ] && apply connection.autoconnect "$v"
          ;;
        prio)
          v=$(ask "Autoconnect priority (higher wins)" "$(get connection.autoconnect-priority)")
          [ -n "$v" ] && apply connection.autoconnect-priority "$v"
          ;;
        psk)
          # key-mgmt is set alongside so a profile saved as open becomes WPA.
          p=$(askpw "New password for $conn")
          [ -n "$p" ] || continue
          report "$conn: password updated" \
            ${nmcli} connection modify "$conn" wifi-sec.key-mgmt wpa-psk wifi-sec.psk "$p"
          ;;
        share)
          # Called (not exec'd) so control returns to this loop afterwards.
          # NET_ROOT is cleared for the child so it does not offer a jump to the
          # root menu: that would exec over the child while this loop is still
          # waiting, leaving the root and the editor stacked. Without it, closing
          # the share view simply falls back into this editor, which is the
          # sensible "back" for a nested view anyway.
          NET_ROOT="" ${sharePass} "$conn"
          ;;
        v4)
          v=$(pick "IPv4 method" auto manual disabled)
          [ -n "$v" ] || continue
          if [ "$v" = manual ]; then
            # NM rejects method=manual unless an address is set in the same
            # call, so collect one first and apply both together.
            a=$(ask "Address (e.g. 192.168.1.50/24)" "$(get ipv4.addresses)")
            [ -n "$a" ] || continue
            report "$conn: static $a" \
              ${nmcli} connection modify "$conn" ipv4.method manual ipv4.addresses "$a"
          else
            apply ipv4.method "$v"
          fi
          ;;
        v6)
          v=$(pick "IPv6 method" auto disabled ignore)
          [ -n "$v" ] && apply ipv6.method "$v"
          ;;
        addr)
          # Only meaningful with ipv4.method=manual (set via the IPv4 entry).
          apply ipv4.addresses "$(ask "Address (e.g. 192.168.1.50/24)" "$(get ipv4.addresses)")"
          ;;
        gw)
          apply ipv4.gateway "$(ask Gateway "$(get ipv4.gateway)")"
          ;;
        dns)
          apply ipv4.dns "$(ask "DNS (comma separated)" "$(get ipv4.dns)")"
          ;;
        rename)
          new=$(ask "New name" "$conn")
          if [ -n "$new" ] && [ "$new" != "$conn" ]; then
            if ${nmcli} connection modify "$conn" connection.id "$new" >/dev/null; then
              ${notify} "Network" "Renamed $conn to $new"
              conn="$new"
            else
              ${notify} -u critical "Network" "Failed to rename $conn"
            fi
          fi
          ;;
        delete)
          if [ "$(pick "Delete $conn?" no yes)" = yes ]; then
            report "Deleted $conn" ${nmcli} connection delete id "$conn"
            exit 0
          fi
          ;;
        *) exit 0 ;;
      esac
    done
  '';

  # Every saved Wi-Fi profile, including ones that are out of range right now.
  # Back returns to $NET_ROOT (see helpers), which avoids this script and the
  # Wi-Fi menu having to refer to each other by name -- Nix cannot evaluate that.
  savedMenu = pkgs.writeShellScript "waybar-wifi-saved" ''
    ${helpers}
    names=() rows=()
    # TYPE first so a NAME containing ':' stays in the trailing field.
    while IFS=: read -r type name; do
      [ "$type" = 802-11-wireless ] || continue
      rows+=("󰖩  ''${name//\\:/:}")
      names+=("''${name//\\:/:}")
    done < <(${nmcli} -g TYPE,NAME connection show)

    if [ "''${#names[@]}" -eq 0 ]; then
      ${notify} "Wi-Fi" "No saved networks"
      exit 0
    fi

    nav_labels=()
    nav_back
    idx=$(printf '%s\n' "''${rows[@]}" | navmenu "Saved networks")
    rc=$?
    # The only toolbar button here is Back.
    [ "$rc" -ge 10 ] && go_back
    [ "$rc" -eq 0 ] && [ -n "$idx" ] && [ "$idx" -ge 0 ] || exit 0
    exec ${editConn} "''${names[$idx]}"
  '';

  wifiMenu = pkgs.writeShellScript "waybar-wifi-menu" ''
    ${helpers}
    ${iconFor}

    # This is a top-level menu, so it is where every Back below returns to.
    export NET_ROOT="$0"

    dev=$(${nmcli} -g DEVICE,TYPE device status | awk -F: '$2=="wifi"{print $1; exit}')
    if [ -z "$dev" ]; then
      ${notify} -u critical "Wi-Fi" "No Wi-Fi device found"
      exit 0
    fi

    # Opening this menu with the radio off is taken as "turn it on": the only
    # useful thing to do here is join a network. Middle-clicking the waybar
    # button is the way to turn it back off, so there is no off switch below.
    if [ "$(${nmcli} radio wifi)" != enabled ]; then
      if ! ${nmcli} radio wifi on >/dev/null 2>&1; then
        ${notify} -u critical "Wi-Fi" "Failed to enable"
        exit 0
      fi
      ${notify} "Wi-Fi" "Enabled"
      # `radio on` returns before the device is ready, so wait for it to leave
      # unavailable; otherwise the scan below runs against a dead device and the
      # menu opens empty.
      for _ in 1 2 3 4 5 6 7 8 9 10; do
        case "$(${nmcli} -g GENERAL.STATE device show "$dev" 2>/dev/null)" in
          *unavailable* | "") sleep 0.5 ;;
          *) break ;;
        esac
      done
      # A freshly woken radio has no results yet, so give the first scan time.
      settle=1.5
    fi

    ssids=() secured=() rows=()
    # IN-USE/ACTIVE are blank on some drivers even while associated, so the
    # device's own active profile name is the reliable source of truth.
    active=$(${nmcli} -g GENERAL.CONNECTION device show "$dev")

    # The radio is guaranteed on by this point, so the list is always scanned.
    ${nmcli} device wifi rescan >/dev/null 2>&1 || true
    [ -n "''${settle-}" ] && sleep "$settle"
    declare -A seen
    # SSID last: terse output escapes ':' inside it as '\:'.
    while IFS=: read -r signal security ssid; do
      ssid="''${ssid//\\:/:}"
      [ -n "$ssid" ] || continue
      [ -z "''${seen[$ssid]-}" ] || continue
      seen["$ssid"]=1
      case "$security" in "" | "--") lock=no ;; *) lock=yes ;; esac
      row="$(icon_for "$signal" "$lock")  $ssid"
      [ "$ssid" = "$active" ] && row="$row  "
      rows+=("$row")
      ssids+=("$ssid") secured+=("$lock")
    done < <(${nmcli} -g SIGNAL,SECURITY,SSID device wifi list ifname "$dev")

    # Toolbar buttons, in kb-custom-1..N order (exit status 10..10+N-1).
    # No "Wi-Fi off" here: middle-clicking the waybar button toggles the radio.
    # The radio is always on by this point (enabled above if it was not), so the
    # list does not vary.
    nav_labels=(
      "󰁐  Rescan" "󰘸  Hidden" "󰖩  Saved" "󰃴  Status"
    )
    nav_keys=(rescan hidden saved status)

    # printf on an empty array would still emit one blank line, so feed nothing.
    if [ "''${#rows[@]}" -gt 0 ]; then
      idx=$(printf '%s\n' "''${rows[@]}" | navmenu "Wi-Fi")
    else
      idx=$(navmenu "Wi-Fi" </dev/null)
    fi
    rc=$?

    # 10+n means a toolbar button was clicked; 0 means a network row was picked.
    if [ "$rc" -ge 10 ]; then
      case "''${nav_keys[rc - 10]}" in
        saved) exec ${savedMenu} ;;
        status) exec ${statusView} ;;
        rescan)
          ${nmcli} device wifi rescan >/dev/null 2>&1
          sleep 2
          exec "$0"
          ;;
        hidden)
          ssid=$(ask "Hidden SSID")
          [ -n "$ssid" ] || exit 0
          pw=$(askpw "Password for $ssid (empty if open)")
          if [ -n "$pw" ]; then
            report "Connected to $ssid" \
              ${nmcli} device wifi connect "$ssid" password "$pw" hidden yes
          else
            report "Connected to $ssid" ${nmcli} device wifi connect "$ssid" hidden yes
          fi
          ;;
      esac
      exit 0
    fi
    # A typed-in entry yields index -1; only a real row selection continues.
    [ "$rc" -eq 0 ] && [ -n "$idx" ] && [ "$idx" -ge 0 ] || exit 0

    ssid="''${ssids[$idx]}"
    if [ "$ssid" = "$active" ]; then
      actions=("disconnect|  Disconnect")
    else
      actions=("connect|󰘋  Connect")
    fi
    # Edit/Forget/Share need a saved profile to act on.
    saved=no
    if ${nmcli} -g NAME connection show | sed 's/\\:/:/g' | grep -Fxq "$ssid"; then
      saved=yes
      actions+=(
        "share|  Show password / QR"
        "edit|󰶓  Edit"
        "forget|  Forget"
      )
    fi
    # Back returns to the network list rather than closing rofi outright.
    actions+=("$(back_entry)")

    case "$(choose "$ssid" "''${actions[@]}")" in
      back) go_back ;;
      disconnect) report "Disconnected from $ssid" ${nmcli} connection down id "$ssid" ;;
      share) exec ${sharePass} "$ssid" ;;
      edit) exec ${editConn} "$ssid" ;;
      forget) report "Forgot $ssid" ${nmcli} connection delete id "$ssid" ;;
      connect)
        if [ "$saved" = yes ]; then
          report "Connected to $ssid" ${nmcli} connection up id "$ssid"
        elif [ "''${secured[$idx]}" = yes ]; then
          pw=$(askpw "Password for $ssid")
          [ -n "$pw" ] || exit 0
          report "Connected to $ssid" \
            ${nmcli} device wifi connect "$ssid" password "$pw" ifname "$dev"
        else
          report "Connected to $ssid" ${nmcli} device wifi connect "$ssid" ifname "$dev"
        fi
        ;;
    esac
  '';

  wifiToggle = pkgs.writeShellScript "waybar-wifi-toggle" ''
    if [ "$(${nmcli} radio wifi)" = enabled ]; then
      ${nmcli} radio wifi off && ${notify} "Wi-Fi" "Disabled"
    else
      ${nmcli} radio wifi on && ${notify} "Wi-Fi" "Enabled"
    fi
  '';

  ethernetMenu = pkgs.writeShellScript "waybar-ethernet-menu" ''
    ${helpers}

    # This is a top-level menu, so it is where every Back below returns to.
    export NET_ROOT="$0"

    devs=() states=() conns=() rows=()
    while IFS=: read -r dev type state conn; do
      [ "$type" = ethernet ] || continue
      if [ "$state" = connected ]; then
        rows+=("󰩁  $dev — $conn")
      else
        rows+=("󰩂  $dev — $state")
      fi
      devs+=("$dev") states+=("$state") conns+=("$conn")
    done < <(${nmcli} -g DEVICE,TYPE,STATE,CONNECTION device status)

    if [ "''${#devs[@]}" -eq 0 ]; then
      ${notify} "Ethernet" "No ethernet devices found"
      exit 0
    fi

    nav_labels=("󰃴  Status")
    nav_keys=(status)
    idx=$(printf '%s\n' "''${rows[@]}" | navmenu "Ethernet")
    rc=$?

    if [ "$rc" -ge 10 ]; then
      case "''${nav_keys[rc - 10]}" in
        status) exec ${statusView} ;;
      esac
      exit 0
    fi
    [ "$rc" -eq 0 ] && [ -n "$idx" ] && [ "$idx" -ge 0 ] || exit 0

    dev="''${devs[$idx]}" conn="''${conns[$idx]}"
    if [ "''${states[$idx]}" = connected ]; then
      actions=("disconnect|  Disconnect")
    else
      actions=("connect|󰸋  Connect")
    fi
    [ -n "$conn" ] && actions+=("edit|󰶓  Edit $conn")
    # Back returns to the device list rather than closing rofi outright.
    actions+=("$(back_entry)")

    case "$(choose "$dev" "''${actions[@]}")" in
      back) go_back ;;
      disconnect) report "$dev disconnected" ${nmcli} device disconnect "$dev" ;;
      connect) report "$dev connected" ${nmcli} device connect "$dev" ;;
      edit) exec ${editConn} "$conn" ;;
    esac
  '';
in
{
  "custom/wifi" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-wifi-poll" ''
      ${iconFor}
      hint="Left: Wi-Fi menu  •  Middle: toggle radio  •  Right: Ethernet"
      if [ "$(${nmcli} radio wifi)" = enabled ]; then
        dev=$(${nmcli} -g DEVICE,TYPE device status | awk -F: '$2=="wifi"{print $1; exit}')
        # Match on SSID rather than IN-USE/ACTIVE, which some drivers leave blank.
        ssid=$(${nmcli} -g GENERAL.CONNECTION device show "$dev" 2>/dev/null)
        signal=$(${nmcli} -g SSID,SIGNAL device wifi list ifname "$dev" 2>/dev/null \
          | sed 's/\\:/:/g' | awk -F: -v s="$ssid" '$1==s{print $2; exit}')
        if [ -n "$ssid" ]; then
          icon="$(icon_for "$signal" no) "
          status="Wi-Fi: $ssid''${signal:+ ($signal%)}"
        else
          icon="󰤯 "
          status="Wi-Fi: On (not connected)"
        fi
      else
        icon="󰤮 "
        status="Wi-Fi: Off"
      fi
      # Escape for the JSON string literals below.
      esc() { printf '%s' "$1" | sed 's/[\\"]/\\&/g'; }
      printf '{"text":"<span size=%s>%s</span>","tooltip":"%s\\n%s"}' \
        "'large'" "$icon" "$(esc "$status")" "$hint"
    '';
    interval = 5;
    on-click = wifiMenu;
    on-click-middle = wifiToggle;
    on-click-right = ethernetMenu;
  };
}
