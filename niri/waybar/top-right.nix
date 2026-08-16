# Top-right modules and their local Rust integrations.
{ pkgs }:
let
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

  # Merged Bluetooth + audio controller. bluer talks to BlueZ over D-Bus and
  # pulsectl-rs talks to PulseAudio, which pipewire-pulse serves, so the two
  # halves need dbus and libpulseaudio to link against.
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
      rofiAudio
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

    # Bluetooth, outputs, and inputs in one module. The text carries the
    # default output's volume glyph plus a Bluetooth glyph; everything else is
    # in the tooltip. Left-click opens the three-tab rofi menu, right-click
    # toggles the Bluetooth adapter.
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
