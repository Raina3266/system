# Top bar modules and their local Rust integrations.
# Merges the former top-left (calendar/system/hardware), top-center (media),
# and top-right (clipboard/network/timer/audio) module groups.
{ pkgs, ycal }:
let
  # Drawer config for system/hardware groups
  drawer = {
    transition-duration = 300;
    transition-left-to-right = true;
  };

  # Package and Waybar integration stay together; only the Rasi theme is kept
  # separately so it remains easy to edit.
  mediaControl = pkgs.rustPlatform.buildRustPackage {
    pname = "media-control";
    version = "0.1.0";
    src = ../../scripts/media-control;
    cargoLock.lockFile = ../../scripts/media-control/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/media-control" \
        --set MEDIA_CONTROL_PLAYERCTL "${pkgs.lib.getExe pkgs.playerctl}" \
        --set MEDIA_CONTROL_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set MEDIA_CONTROL_FALLBACK_THEME "$out/share/rofi/themes/media-control.rasi"
    '';
  };

  previewPanel = pkgs.rustPlatform.buildRustPackage {
    pname = "preview-panel";
    version = "0.1.0";
    src = ../../scripts/preview-panel;
    cargoLock.lockFile = ../../scripts/preview-panel/Cargo.lock;

    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wrapGAppsHook4
    ];
    buildInputs = [
      pkgs.gtk4
      pkgs.gtk4-layer-shell
    ];
  };

  rofiClipboard = pkgs.rustPlatform.buildRustPackage {
    pname = "rofi-clipboard";
    version = "0.1.0";
    src = ../../scripts/rofi-clipboard;
    cargoLock.lockFile = ../../scripts/rofi-clipboard/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-clipboard" \
        --set ROFI_CLIPBOARD_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_CLIPBOARD_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_CLIPBOARD_WL_COPY "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-copy"}" \
        --set ROFI_CLIPBOARD_WL_PASTE "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-paste"}"
    '';
  };

  rofiNetworkManager = pkgs.rustPlatform.buildRustPackage {
    pname = "rofi-network-manager";
    version = "0.1.0";
    src = ../../scripts/rofi-network-manager;
    cargoLock.lockFile = ../../scripts/rofi-network-manager/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-network-manager" \
        --set ROFI_NETWORK_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_NETWORK_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_NETWORK_NMCLI "${pkgs.lib.getExe' pkgs.networkmanager "nmcli"}" \
        --set ROFI_NETWORK_QRENCODE "${pkgs.lib.getExe' pkgs.qrencode "qrencode"}" \
        --set ROFI_NETWORK_ROFI_WIDTH "350"
    '';
  };

  rofiAudio = pkgs.rustPlatform.buildRustPackage {
    pname = "rofi-audio";
    version = "0.1.0";
    src = ../../scripts/rofi-audio;
    cargoLock.lockFile = ../../scripts/rofi-audio/Cargo.lock;

    nativeBuildInputs = [
      pkgs.makeWrapper
      pkgs.pkg-config
    ];
    buildInputs = [
      pkgs.dbus
      pkgs.libpulseaudio
    ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-audio" \
        --set ROFI_AUDIO_ROFI "${pkgs.lib.getExe pkgs.rofi}"
    '';
  };

  waybarTimer = pkgs.rustPlatform.buildRustPackage {
    pname = "waybar-timer";
    version = "0.1.0";
    src = ../../scripts/waybar-timer;
    cargoLock.lockFile = ../../scripts/waybar-timer/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/waybar-timer" \
        --set WAYBAR_TIMER_FFPLAY "${pkgs.ffmpeg}/bin/ffplay"
    '';
  };

  # Treat each media tab as an MPRIS player.
  mprisenceNativeHost =
    pkgs.writeTextDir "etc/chromium/native-messaging-hosts/mprisence.web.bridge.json"
      (
        builtins.toJSON {
          name = "mprisence.web.bridge";
          description = "Publish each browser media tab as an MPRIS player";
          path = "${pkgs.mprisence}/bin/mprisence";
          type = "stdio";
          allowed_origins = [
            "chrome-extension://pnkkjbdopihogobhhjbgapbpfccinjjo/"
            "chrome-extension://pphdmbejbipjlocngoefnmjoijcbdejf/"
          ];
        }
      );
in
{
  homeConfig = {
    home.packages = [
      pkgs.wl-clipboard
      previewPanel
      rofiClipboard
      rofiNetworkManager
      rofiAudio
      pkgs.mprisence
    ];
    programs.google-chrome = {
      commandLineArgs = [
        "--disable-features=HardwareMediaKeyHandling,MediaSessionService"
      ];
      nativeMessagingHosts = [ mprisenceNativeHost ];
    };
  };

  modules = {
    # --- formerly top-left: calendar, system/hardware groups ---

    "custom/ycal" = {
      return-type = "json";
      interval = 60;
      exec = ycal.ycalBarExec;
      on-click = ycal.ycalToggle;
    };

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

    # thermal-zone 8 is x86_pkg_temp, the CPU package sensor. Only
    # {temperatureC}/{temperatureF}/{temperatureK} are valid placeholders
    # here; anything else makes the module throw on every poll.
    "temperature" = {
      thermal-zone = 8;
      warning-threshold = 55;
      critical-threshold = 80;
      interval = 5;
      format = "󰄏 {temperatureC}°C";
      format-critical = "󰄅 {temperatureC}°C";
      tooltip-format = "CPU package: {temperatureC}°C";
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

    # --- formerly top-center: media and lyrics ---

    "custom/media" = {
      hide-empty-text = true;
      format = "{}";
      return-type = "json";
      exec = "${mediaControl}/bin/media-control waybar --watch --interval-ms 750";
      tooltip = true;
      escape = true;
      "restart-interval" = 2;
      "exec-on-event" = false;

      # Left toggles the player shown in Waybar; right opens the full controller.
      on-click = "${mediaControl}/bin/media-control toggle";
      on-click-right = "${mediaControl}/bin/media-control menu";
    };

    "custom/lyrics" = {
      hide-empty-text = true;
      return-type = "json";
      format = "󰝚  {text}";
      exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
      exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
      on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
    };

    # --- formerly top-right: utilities ---

    "custom/network" = {
      exec = "${rofiNetworkManager}/bin/rofi-network-manager status";
      interval = 5;
      return-type = "json";
      tooltip = true;
      escape = false;
      on-click = "${rofiNetworkManager}/bin/rofi-network-manager";
    };

    "custom/timer" = {
      exec = "${waybarTimer}/bin/waybar-timer";
      format = "{}";
      return-type = "json";
      tooltip = true;
      escape = false;
      "restart-interval" = 1;
      "exec-on-event" = false;
      on-click = "${waybarTimer}/bin/waybar-timer add";
      on-click-middle = "${waybarTimer}/bin/waybar-timer toggle";
      on-click-right = "${waybarTimer}/bin/waybar-timer clear";
    };

    "custom/clipboard" = {
      exec = "${rofiClipboard}/bin/rofi-clipboard status";
      return-type = "json";
      tooltip = true;
      escape = false;
      "restart-interval" = 1;
      "exec-on-event" = false;
      on-click = "${rofiClipboard}/bin/rofi-clipboard";
      on-click-right = "${rofiClipboard}/bin/rofi-clipboard clear";
    };

    "custom/audio" = {
      exec = "${rofiAudio}/bin/rofi-audio status";
      interval = 5;
      return-type = "json";
      tooltip = true;
      escape = false;
      on-click = "${rofiAudio}/bin/rofi-audio";
      on-click-right = "${rofiAudio}/bin/rofi-audio bluetooth-power toggle";
    };

    "tray" = {
      icon-size = 18;
      spacing = 10;
    };
  };
}
