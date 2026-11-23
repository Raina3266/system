{
  pkgs,
  mkWorkspacePackage,
  mkPanel,
  repoRoot,
  palette,
  astalPackages,
}:
let
  dashboardControl = mkWorkspacePackage "dashboard-control" {
    workspaceMember = "dashboard/rust-custom";
    passthru = { inherit panel; };
  };

  panel = mkPanel {
    inherit palette;
    name = "dashboard-panel";
    prefix = "DASHBOARD_PANEL";
    sourceDir = "dashboard/astal";
    cssFile = "${repoRoot}/scripts/dashboard/astal/style.css";

    extraPackages = [
      astalPackages.battery astalPackages.powerprofiles astalPackages.notifd
    ];
    imageLoaders = true;
    environment = {
      NETWORK_PANEL = "network-panel";
      DASHBOARD_CONTROL = pkgs.lib.getExe dashboardControl;
      SWAYIDLE = pkgs.lib.getExe pkgs.swayidle;
      SYSTEMCTL = "${pkgs.systemd}/bin/systemctl";
    };
    commands = [ "run" "status" "toggle" "close" "toggleDnd" "clearNotifications" ];
    passthru.homeManagerModule =
      {
        config,
        lib,
        repoPackages,
        ...
      }:
      {
        home.packages = [ repoPackages.dashboardPanel repoPackages.dashboardControl ];

        xdg.configFile."ags/dashboard-palette.json".source = repoPackages.dashboardPanel.palette;

        systemd.user.services.ags-dashboard = {
          Unit = {
            Description = "AGS/Astal GTK4 dashboard and notification server";
            Conflicts = [ "swaync.service" ];
            X-Restart-Triggers = [ config.xdg.configFile."ags/dashboard-palette.json".source ];
            ConditionEnvironment = [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            Type = "dbus";
            BusName = "org.freedesktop.Notifications";
            ExecStart = "${lib.getExe repoPackages.dashboardPanel} run";
            Environment = [
              "GDK_BACKEND=wayland"

              "DASHBOARD_PANEL_PALETTE=${config.xdg.configHome}/ags/dashboard-palette.json"
              "NETWORK_PANEL=${lib.getExe repoPackages.networkPanel}"
              "DASHBOARD_CONTROL=${lib.getExe repoPackages.dashboardControl}"
              "NOTIFICATION_HISTORY=%S/dashboard/notifications.json"
              "LEGACY_NOTIFICATION_HISTORY=%S/quickshell/notifications.json"
            ];
            StateDirectory = "dashboard";
            StateDirectoryMode = "0700";
            UMask = "0077";
            Restart = "on-failure";
            RestartSec = 3;
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };
      };
  };
in
dashboardControl
