{
  pkgs,
  mkWorkspacePackage,
  mkPanel,
  repoRoot,
  palette,
}:
let
  backend = mkWorkspacePackage "network-control" {
    workspaceMember = "network-manager/rust-custom";
    nativeCheckInputs = [ (pkgs.lib.getBin pkgs.dbus) ];
  };
in
mkPanel {
  inherit palette;
  name = "network-panel";
  prefix = "NETWORK_PANEL";
  sourceDir = "network-manager";
  cssFile = "${repoRoot}/scripts/network-manager/style.css";
  environment = {
    NETWORK_CONTROL = pkgs.lib.getExe backend;
    DASHBOARD_PANEL = "dashboard-panel";
    MEDIA_PANEL = "media-panel";
  };
  commands = [ "run" "status" "toggle" "close" "toggleWifi" ];
  passthru.backend = backend;
  passthru.homeManagerModule =
    {
      config,
      lib,
      repoPackages,
      ...
    }:
    {
      home.packages = [ repoPackages.networkPanel repoPackages.networkControl ];

      xdg.configFile."ags/network-palette.json".source = repoPackages.networkPanel.palette;

      systemd.user.services.ags-network = {
        Unit = {
          Description = "AGS/Astal GTK4 network panel";
          X-Restart-Triggers = [ config.xdg.configFile."ags/network-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.networkPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"

            "NETWORK_PANEL_PALETTE=${config.xdg.configHome}/ags/network-palette.json"

            "DASHBOARD_PANEL=${lib.getExe repoPackages.dashboardPanel}"
            "MEDIA_PANEL=${lib.getExe repoPackages.mediaPanel}"
            "NETWORK_CONTROL=${lib.getExe repoPackages.networkControl}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };
    };
}
