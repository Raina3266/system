# Waybar configuration: top + bottom bars with the cyberpunk theme.
#
# The top bar (clock, hardware, media, utilities) and bottom bar
# (niri workspace taskbar + launcher buttons) live in separate files
# for readability. Polling scripts (todo, timer) are in scripts.nix.
{
  pkgs,
  lib,
  config,
  osConfig,
  ...
}:
let
  cfg = config.programs'.waybar;
  scripts = import ./scripts.nix { inherit pkgs; };

  # Outputs to attach the bars to: every non-auxiliary display declared
  # in osConfig.services'.desktop.displays (if any).
  barOutputs = lib.optionalAttrs ((osConfig.services'.desktop.displays or [ ]) != [ ]) {
    output = map (d: d.name) (lib.filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
  };

  topBar = (import ./top.nix { inherit pkgs scripts; }) // barOutputs;
  bottomBar = (import ./bottom.nix { }) // barOutputs;
in
{
  options.programs'.waybar = {
    enable = lib.mkEnableOption "waybar";
    enableNiriIntegration = lib.mkEnableOption "Niri workspace switcher";
  };

  config = lib.mkIf (pkgs.stdenv.isLinux && cfg.enable) (
    lib.mkMerge [
      {
        home.packages = with pkgs; [
          waybar-lyric
          wl-clipboard
          jq
          playerctl
        ];

        systemd.user.services.waybar = {
          Unit = {
            # Only run under niri — GNOME/Mutter lacks layer-shell support
            # and waybar would crash-loop there.
            ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
          };
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };
      }

      (lib.mkIf (osConfig != null) {
        programs.waybar = {
          enable = true;
          systemd.enable = true;
          style = ../themes/waybar-cyberpunk.css;

          settings = {
            inherit topBar bottomBar;
          };
        };
      })
    ]
  );
}
