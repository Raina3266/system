{
  pkgs,
  mkWorkspacePackage,
  mkCargoArtifacts,
  mkPanel,
  repoRoot,
  palette,
}:
let
  clipboardCustom = mkWorkspacePackage "clipboard-custom" {
    workspaceMember = "clipboard/rust-custom";
    nativeCheckInputs = [ pkgs.glib.bin ];
    postInstall = ''
      wrapProgram "$out/bin/clipboard-custom" \
        --set CLIPBOARD_GIO "${pkgs.glib.bin}/bin/gio" \
        --set CLIPBOARD_WL_PASTE "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-paste"}"
    '';
    cargoArtifacts = mkCargoArtifacts "clipboard-custom-deps" {
      cargoExtraArgs = "--locked --package clipboard-custom";
    };
    passthru = { inherit panel; };
  };

  panel = mkPanel {
    inherit palette;
    name = "clipboard-panel";
    prefix = "CLIPBOARD_PANEL";
    sourceDir = "clipboard/astal";
    cssFile = "${repoRoot}/scripts/clipboard/astal/style.css";
    imageLoaders = true;
    strictHelp = false;
    checkResponse = true;
    environment.CLIPBOARD_CUSTOM_HELPER = pkgs.lib.getExe clipboardCustom;
    passthru.homeManagerModule =
      {
        config,
        lib,
        pkgs,
        repoPackages,
        ...
      }:
      {
        home.packages = [
          repoPackages.clipboardPanel
          repoPackages.clipboardCustom
        ];

        xdg.configFile."ags/clipboard-palette.json".source = repoPackages.clipboardPanel.palette;

        # One data-control watcher per session, independent of panels and Waybar.
        systemd.user.services.clipboard-collector = {
          Unit = {
            Description = "Capture Wayland clipboard history once per session";
            ConditionEnvironment = [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${lib.getExe' pkgs.wl-clipboard "wl-paste"} --no-newline --watch ${lib.getExe repoPackages.clipboardCustom} capture";
            UMask = "0077";
            Restart = "always";
            RestartSec = 2;
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };

        systemd.user.services.ags-clipboard = {
          Unit = {
            Description = "AGS/Astal GTK4 clipboard history and preview panel";
            X-Restart-Triggers = [ config.xdg.configFile."ags/clipboard-palette.json".source ];
            ConditionEnvironment = [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            PartOf = [ "graphical-session.target" ];
            Wants = [ "clipboard-collector.service" ];
            After = [ "graphical-session.target" "clipboard-collector.service" ];
          };
          Service = {
            ExecStart = "${lib.getExe repoPackages.clipboardPanel} run";
            Environment = [
              "GDK_BACKEND=wayland"

              "CLIPBOARD_PANEL_PALETTE=${config.xdg.configHome}/ags/clipboard-palette.json"
            ];
            UMask = "0077";
            Restart = "on-failure";
            RestartSec = 3;
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };
      };
  };
in
clipboardCustom
