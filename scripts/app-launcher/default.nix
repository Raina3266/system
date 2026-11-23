{
  pkgs,
  mkPanel,
  repoRoot,
  astalPackages,
  panelLauncher,
}:
mkPanel {
  name = "app-launcher";
  prefix = "APP_LAUNCHER";
  sourceDir = "app-launcher";
  cssFile = "${repoRoot}/scripts/app-launcher/style.css";
  extraPackages = [ astalPackages.apps ];
  withPalette = false;
  environment = {
    NIRI = pkgs.lib.getExe pkgs.niri;
    PANEL_BACKEND = pkgs.lib.getExe panelLauncher;
  };
  commands = [ "run" "toggle" "close" "status" ];
  passthru.homeManagerModule =
    {
      lib,
      pkgs,
      repoPackages,
      ...
    }:
    {
      home.packages = [ repoPackages.appLauncher ];

      systemd.user.services.ags-app-launcher = {
        Unit = {
          Description = "AGS/Astal GTK4 app launcher";
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.appLauncher} run";
          Environment = [
            "GDK_BACKEND=wayland"

            "NIRI=${lib.getExe pkgs.niri}"
            "PANEL_BACKEND=${lib.getExe repoPackages.panelLauncher}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };
    };
}
