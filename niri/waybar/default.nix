# Waybar: top and bottom status bars with cyberpunk theme.
# Bar layouts: layout.nix (top: clock/hardware/media/utilities; bottom: taskbar)
# Local Rust packages: media, clipboard, network, and timer in top.nix
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
  top = import ./top.nix { inherit pkgs ycal; };
  layout = import ./layout.nix { inherit pkgs top; };

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

  config = lib.mkIf (pkgs.stdenv.hostPlatform.isLinux && cfg.enable) (
    lib.mkMerge [
      top.homeConfig

      {
        home.packages = with pkgs; [
          waybar-lyric
          ycal.waybarYcal
          jq
          playerctl
        ];
      }

      (lib.mkIf (osConfig != null) {
        programs.waybar = {
          enable = true;

          settings = {
            inherit topBar bottomBar;
          };
        };

        # Style is symlinked directly to the repo's waybar.css for live editing
        xdg.configFile."waybar/style.css".source =
          config.lib.file.mkOutOfStoreSymlink "/home/raina/System/niri/themes/waybar.css";
      })
    ]
  );
}
