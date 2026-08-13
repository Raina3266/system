# Waybar: top and bottom status bars with cyberpunk theme.
# Bar layouts: layout.nix (top: clock/hardware/media/utilities; bottom: taskbar)
# Local Rust packages: media in top-center.nix; clipboard and timer in top-right.nix
{
  pkgs,
  lib,
  config,
  osConfig,
  ...
}:
let
  cfg = config.programs'.waybar;
  ycal = import ./calender.nix { inherit pkgs; };
  topRight = import ./top-right.nix { inherit pkgs config; };
  layout = import ./layout.nix { inherit pkgs topRight; };

  # Bar outputs: non-auxiliary displays from osConfig.services'.desktop.displays
  barOutputs = lib.optionalAttrs ((osConfig.services'.desktop.displays or [ ]) != [ ]) {
    output = map (d: d.name) (lib.filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
  };

  topBar = layout.topBar // barOutputs;
  bottomBar = layout.bottomBar // barOutputs;
in
{
  options.programs'.waybar = {
    enable = lib.mkEnableOption "waybar";
  };

  config = lib.mkIf (pkgs.stdenv.isLinux && cfg.enable) (
    lib.mkMerge [
      topRight.homeConfig

      {
        home.packages = with pkgs; [
          waybar-lyric
          ycal.waybarYcal
          jq
          playerctl
        ];

        systemd.user.services.waybar = {
          Unit = {
            # Only run under niri (GNOME/Mutter lacks layer-shell support)
            ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
          };
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };

        systemd.user.services.waybar-ycal = {
          Unit = {
            Description = "waybar-ycal: Google Calendar and Tasks popup";
            ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${ycal.waybarYcal}/bin/waybar-ycal-popup";
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
          Install = {
            WantedBy = [ "graphical-session.target" ];
          };
        };
      }

      (lib.mkIf (osConfig != null) {
        programs.waybar = {
          enable = true;
          systemd.enable = true;

          settings = {
            inherit topBar bottomBar;
          };
        };

        # Style is symlinked directly to the repo's waybar.css for live editing
        xdg.configFile."waybar/style.css".source =
          config.lib.file.mkOutOfStoreSymlink "/home/raina/System/home/niri/themes/waybar.css";
      })
    ]
  );
}
