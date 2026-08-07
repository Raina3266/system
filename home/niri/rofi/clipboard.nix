{ pkgs, ... }:
let
  rofiClipboard = pkgs.rustPlatform.buildRustPackage {
    pname = "rofi-clipboard";
    version = "0.1.0";

    # This module belongs at home/niri/rofi-clipboard.nix.
    src = ../../../scripts/rofi-clipboard;
    cargoLock.lockFile = ../../../scripts/rofi-clipboard/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram "$out/bin/rofi-clipboard" \
        --set ROFI_CLIPBOARD_ROFI "${pkgs.rofi}/bin/rofi" \
        --set ROFI_CLIPBOARD_WL_COPY "${pkgs.wl-clipboard}/bin/wl-copy" \
        --set ROFI_CLIPBOARD_WL_PASTE "${pkgs.wl-clipboard}/bin/wl-paste"
    '';
  };
in
{
  home.packages = [ rofiClipboard ];

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
