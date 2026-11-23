{
  pkgs,
  mkWorkspacePackage,
  mkPanel,
  repoRoot,
  palette,
  astalPackages,
}:
let
  # Resident media policy/metadata and private MPRIS recovery.
  mediaControl = mkWorkspacePackage "media-control" {
    workspaceMember = "media-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/media-control" \
        --set MEDIA_CONTROL_PLAYERCTL "${pkgs.playerctl}/bin/playerctl" \
        --set MEDIA_CONTROL_CURL "${pkgs.lib.getExe pkgs.curl}"
    '';
    passthru = { inherit panel; };
  };

  panel = mkPanel {
    inherit palette;
    name = "media-panel";
    prefix = "MEDIA_PANEL";
    sourceDir = "media-control/astal";
    cssFile = "${repoRoot}/scripts/media-control/astal/style.css";
    extraPackages = [ astalPackages.mpris astalPackages.wireplumber ];
    imageLoaders = true;
    strictHelp = false;
    environment = {
      MEDIA_CONTROL = pkgs.lib.getExe mediaControl;
      NETWORK_PANEL = "network-panel";
    };
    commands = [ "run" "status" "toggle" "togglePauseAll" "cycleMedia" "pauseAll" "close" ];
    passthru.homeManagerModule =
      {
        config,
        lib,
        repoPackages,
        ...
      }:
      {
        home.packages = [
          repoPackages.mediaPanel
          repoPackages.mediaControl
        ];

        xdg.configFile."ags/media-palette.json".source = repoPackages.mediaPanel.palette;

        systemd.user.services.ags-media = {
          Unit = {
            Description = "AGS/Astal GTK4 media panel";
            X-Restart-Triggers = [ config.xdg.configFile."ags/media-palette.json".source ];
            ConditionEnvironment = [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${lib.getExe repoPackages.mediaPanel} run";
            Environment = [
              "GDK_BACKEND=wayland"

              "MEDIA_PANEL_PALETTE=${config.xdg.configHome}/ags/media-palette.json"
              "MEDIA_CONTROL=${lib.getExe repoPackages.mediaControl}"
              "NETWORK_PANEL=${lib.getExe repoPackages.networkPanel}"
            ];
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };
      };
  };
in
mediaControl
