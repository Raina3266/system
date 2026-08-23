# Waybar: top and bottom status bars with cyberpunk theme.
{
  config,
  lib,
  osConfig,
  pkgs,
  ...
}:
let
  cfg = config.programs'.waybar;
  packages = import ../../packages.nix { inherit pkgs; };

  system = import ./system.nix { inherit pkgs packages; };
  utilities = import ./utilities.nix { inherit pkgs packages; };
  calendar = import ./calendar.nix { inherit lib packages; };
  taskbar = import ./taskbar.nix { inherit packages; };

  modules = system.modules // utilities.modules // calendar.modules;
  layout = import ./layout.nix {
    inherit pkgs modules taskbar;
  };

  displays = if osConfig == null then [ ] else osConfig.services'.desktop.displays or [ ];
  barOutputs = lib.optionalAttrs (displays != [ ]) {
    output = map (display: display.name) (lib.filter (display: !display.auxiliary) displays);
  };
  topBar = layout.topBar // barOutputs;
  bottomBar = layout.bottomBar // barOutputs;
  themeRoot = "${config.home.homeDirectory}/System/themes";
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

        xdg.configFile."waybar/style.css".source =
          config.lib.file.mkOutOfStoreSymlink "${themeRoot}/waybar.css";
      })
    ]
  );
}
