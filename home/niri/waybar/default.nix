# Waybar: top and bottom status bars with cyberpunk theme.
# Top bar: clock, hardware, media, utilities (see top.nix)
# Bottom bar: niri workspace taskbar + launcher buttons (see bottom.nix)
# Polling scripts: todo/timer in top-right.nix
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

  # Bar outputs: non-auxiliary displays from osConfig.services'.desktop.displays
  barOutputs = lib.optionalAttrs ((osConfig.services'.desktop.displays or [ ]) != [ ]) {
    output = map (d: d.name) (lib.filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
  };

  topBar = (import ./top.nix { inherit pkgs; }) // barOutputs;
  bottomBar = (import ./bottom.nix { inherit pkgs; }) // barOutputs;
in
{
  options.programs'.waybar = {
    enable = lib.mkEnableOption "waybar";
  };

  config = lib.mkIf (pkgs.stdenv.isLinux && cfg.enable) (
    lib.mkMerge [
      {
        home.packages = with pkgs; [
          waybar-lyric
          ycal.waybarYcal
          wl-clipboard
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
          Install = { WantedBy = [ "graphical-session.target" ]; };
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

        # Style is a symlink to the repo (see ../themes/waybar-style.css),
        # so edits apply without a rebuild. Each imported stylesheet is also
        # symlinked next to it, since GTK resolves @import relative to the
        # loaded file's directory (~/.config/waybar/), not the link target.
        xdg.configFile =
          let
            waybarCss =
              name:
              lib.nameValuePair "waybar/${name}" {
                source = config.lib.file.mkOutOfStoreSymlink "/home/raina/System/home/niri/themes/${name}";
              };
          in
          lib.listToAttrs (map waybarCss [
            "waybar.css"
            "waybar-bottom.css"
          ])
          // {
            "waybar/style.css".source =
              config.lib.file.mkOutOfStoreSymlink
                "/home/raina/System/home/niri/themes/waybar-style.css";
          };
      })
    ]
  );
}
