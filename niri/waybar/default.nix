# Waybar: top and bottom status bars with cyberpunk theme.
{
  config,
  lib,
  osConfig,
  pkgs,
  repoRoot,
  repoPackages,
  ...
}:
let
  cfg = config.programs'.waybar;

  system = import ./system.nix {
    inherit lib pkgs;
    packages = repoPackages;
  };
  utilities = import ./utilities.nix {
    inherit pkgs;
    packages = repoPackages;
  };
  modules = system.modules // utilities.modules;
  taskbar = utilities.taskbar;

  # ------------ Waybar layouts ------------
  common = {
    layer = "top";
    height = 40;
    smooth-scrolling-threshold = 5;
    reload_style_on_change = true;
  };

  # ------------ TopBar -------------

  topBar =
    common
    // {
      include = [ "${repoRoot}/themes/waybar/top.jsonc" ];
    }
    // modules;

  wayleOutputs = [
    "eDP-1"
    "DP-8"
    "DP-7"
  ];

  topBars = builtins.listToAttrs (
    map (output: {
      name = "topBar-${output}";
      value = topBar // {
        inherit output;
        "custom/audio" = system.audioModule output;
        "custom/dashboard" = system.dashboardModule output;
        "custom/network" = system.networkModule output;
        "custom/wayle-media" = system.wayleMediaModule output;
      };
    }) wayleOutputs
  );

  # ------------ BottomBar -------------

  # No `output`: one config shown on every monitor. The taskbar sizes itself
  # per monitor through max_taskbar_width_per_output.
  bottomBar =
    common
    // {
      include = [ "${repoRoot}/themes/waybar/bottom.jsonc" ];
    }
    // taskbar;
in
{
  options.programs'.waybar.enable = lib.mkEnableOption "waybar";

  config = lib.mkIf (pkgs.stdenv.hostPlatform.isLinux && cfg.enable) (
    lib.mkMerge [
      system.homeConfig
      utilities.homeConfig

      {
        home.packages = with pkgs; [
          waybar-lyric
          jq
          playerctl
        ];
      }

      (lib.mkIf (osConfig != null) {
        programs.waybar = {
          enable = true;
          systemd.enable = true;
          settings = topBars // { inherit bottomBar; };
        };

        systemd.user.services.waybar = {
          Unit = {
            ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
            Wants = [
              "waybar-timer.service"
              "rofi-clipboard-collector.service"
            ];
            After = [
              "waybar-timer.service"
              "rofi-clipboard-collector.service"
            ];
          };
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };

        # Waybar does not watch its JSON configuration. Restart it whenever a
        # live layout fragment is saved; CSS reloads natively via the setting
        # above and does not need a system rebuild either.
        systemd.user.paths.waybar-live-layout = {
          Unit.Description = "Watch live Waybar layout files";
          Path = {
            PathChanged = [
              "${repoRoot}/themes/waybar/top.jsonc"
              "${repoRoot}/themes/waybar/bottom.jsonc"
            ];
            Unit = "waybar-live-layout.service";
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };

        systemd.user.services.waybar-live-layout = {
          Unit.Description = "Apply live Waybar layout changes";
          Service = {
            Type = "oneshot";
            ExecStart = "${pkgs.systemd}/bin/systemctl --user try-restart waybar.service";
          };
        };
      })
    ]
  );
}
