# Waybar: top and bottom status bars with cyberpunk theme.
{
  config,
  lib,
  osConfig,
  pkgs,
  repoPackages,
  ...
}:
let
  cfg = config.programs'.waybar;

  system = import ./system.nix {
    inherit pkgs;
    packages = repoPackages;
  };
  utilities = import ./utilities.nix {
    inherit pkgs;
    packages = repoPackages;
  };
  calendar = import ./calendar.nix {
    inherit lib;
    packages = repoPackages;
  };
  taskbar = import ./taskbar.nix { packages = repoPackages; };

  modules = system.modules // utilities.modules // calendar.modules;
  layout = import ./layout.nix {
    inherit modules taskbar;
  };

  displays = if osConfig == null then [ ] else osConfig.services'.desktop.displays or [ ];
  barOutputs = lib.optionalAttrs (displays != [ ]) {
    output = map (display: display.name) (lib.filter (display: !display.auxiliary) displays);
  };
  topBar = layout.topBar // barOutputs;
  bottomBar = layout.bottomBar // barOutputs;
in
{
  options.programs'.waybar.enable = lib.mkEnableOption "waybar";

  config = lib.mkIf (pkgs.stdenv.hostPlatform.isLinux && cfg.enable) (
    lib.mkMerge [
      system.homeConfig
      utilities.homeConfig
      calendar.homeConfig

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
          settings = {
            inherit topBar bottomBar;
          };
        };

        systemd.user.services.waybar = {
          Unit.ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };
      })
    ]
  );
}
