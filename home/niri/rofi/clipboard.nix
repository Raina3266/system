# Packages, wrappers, service, and runtime links for the clipboard stack.
# UI code stays in the Rust sources, Rasi theme, and preview-panel config.
{ pkgs, config, ... }:
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

  # Keep theme and preview styling live-editable without rebuilding.
  xdg.configFile."rofi/rofi-clipboard.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/theme/rofi-clipboard.rasi";
  xdg.configFile."preview-panel/config.toml".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/scripts/preview-panel/config.toml";

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

    Install.WantedBy = [ "graphical-session.target" ];
  };
}
