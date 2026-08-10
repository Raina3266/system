{ pkgs, ... }:
let
  previewPanel = pkgs.rustPlatform.buildRustPackage {
    pname = "preview-panel";
    version = "0.1.0";
    
    src = ../../../scripts/preview-panel;
    cargoLock.lockFile = ../../../scripts/preview-panel/Cargo.lock;

    nativeBuildInputs = [
      pkg-config
      wrapGAppsHook4
    ];
    buildInputs = [ gtk4 ];

  };

  rofiClipboard = pkgs.rustPlatform.buildRustPackage {
    pname = "rofi-clipboard";
    version = "0.1.0";

    src = ../../../scripts/rofi-clipboard;
    cargoLock.lockFile = ../../../scripts/rofi-clipboard/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-clipboard" \
        --set ROFI_CLIPBOARD_ROFI "${pkgs.rofi}/bin/rofi" \
        --set ROFI_CLIPBOARD_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_CLIPBOARD_WL_COPY "${pkgs.wl-clipboard}/bin/wl-copy" \
        --set ROFI_CLIPBOARD_WL_PASTE "${pkgs.wl-clipboard}/bin/wl-paste"
    '';
  };
in
{
  home.packages = [
    previewPanel
    rofiClipboard
  ];

  systemd.user.services.rofi-clipboard = {
    Unit = {
      Description = "Rofi clipboard history collector";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };

    Service = {
      ExecStart = "${pkgs.wl-clipboard}/bin/wl-paste --watch ${rofiClipboard}/bin/rofi-clipboard capture";
      Restart = "on-failure";
      RestartSec = 2;
    };

    Install = {
      WantedBy = [ "graphical-session.target" ];
    };
  };
}
