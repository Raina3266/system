# Top-right modules and their local Rust integrations.
{ pkgs }:
let
  previewPanel = pkgs.rustPlatform.buildRustPackage {
    pname = "preview-panel";
    version = "0.1.0";
    src = ../../../scripts/preview-panel;
    cargoLock.lockFile = ../../../scripts/preview-panel/Cargo.lock;

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
    src = ../../../scripts/rofi-clipboard;
    cargoLock.lockFile = ../../../scripts/rofi-clipboard/Cargo.lock;

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
    src = ../../../scripts/rofi-network-manager;
    cargoLock.lockFile = ../../../scripts/rofi-network-manager/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-network-manager" \
        --set ROFI_NETWORK_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_NETWORK_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_NETWORK_NMCLI "${pkgs.lib.getExe' pkgs.networkmanager "nmcli"}" \
        --set ROFI_NETWORK_QRENCODE "${pkgs.lib.getExe' pkgs.qrencode "qrencode"}" \
        --set ROFI_NETWORK_ROFI_WIDTH "650"
    '';
  };

  waybarTimer = pkgs.rustPlatform.buildRustPackage {
    pname = "waybar-timer";
    version = "0.1.0";
    src = ../../../scripts/waybar-timer;
    cargoLock.lockFile = ../../../scripts/waybar-timer/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/waybar-timer" \
        --set WAYBAR_TIMER_FFPLAY "${pkgs.ffmpeg}/bin/ffplay"
    '';
  };

  walker = "${pkgs.walker}/bin/walker";
  inherit ((import ../walker/bluetooth.nix { inherit pkgs; })) btctl;

  # Static launcher icons with walker integration
  staticLauncher = name: icon: tooltip: walkerArgs: {
    format = "<span size='large'>${icon}</span>";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-${name}-poll" ''
      printf '{"text":"<span size='"'"'large'"'"'>${icon}</span>","tooltip":"${tooltip}"}'
    '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-${name}" ''
      ${walker} ${walkerArgs}
    '';
  };
in
{
  # Package installation, service, and runtime links stay beside the Waybar
  # launcher. Only the editable theme and preview format source files live
  # elsewhere.
  homeConfig = {
    home.packages = [
      pkgs.wl-clipboard
      previewPanel
      rofiClipboard
      rofiNetworkManager
    ];

    systemd.user.services.rofi-clipboard = {
      Unit = {
        Description = "Rofi clipboard history collector";
        ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };

      Service = {
        ExecStart = "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-paste"} --watch ${rofiClipboard}/bin/rofi-clipboard capture";
        Restart = "on-failure";
        RestartSec = 2;
      };

      Install.WantedBy = [ "graphical-session.target" ];
    };
  };

  modules = {
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
      format = "<span size='large'>󰍩</span>";
      tooltip-format = "Clipboard";
      on-click = "${rofiClipboard}/bin/rofi-clipboard";
    };

    # Audio: opens walker device picker (volume in pulseaudio module)
    "custom/audio" = staticLauncher "audio" "󰕾" "Audio devices & volume" "-n -m menus:audio";

    "tray" = {
      icon-size = 18;
      spacing = 10;
    };

    "custom/bt" = {
      format = "{}";
      return-type = "json";
      exec = pkgs.writeShellScript "waybar-bt-poll" ''
        on_icon="<span size='large'>󰂯</span>"
        off_icon="<span size='large'>󰂱</span>"
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
      # Left: open walker bluetooth menu | Right: toggle power
      # Uses btctl (D-Bus) to avoid bluetoothctl agent conflicts
      on-click = pkgs.writeShellScript "waybar-bt" ''
        ${walker} -n -m menus:bluetooth
      '';
      on-click-right = pkgs.writeShellScript "waybar-bt-toggle-power" ''
        powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
        if [ "$powered" = "yes" ]; then
          ${btctl}/bin/btctl power off
        else
          ${btctl}/bin/btctl power on
        fi
      '';
    };
  };
}
