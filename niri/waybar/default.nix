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
    inherit lib pkgs;
    packages = repoPackages;
  };
  utilities = import ./utilities.nix {
    packages = repoPackages;
  };
  modules = system.modules // utilities.modules;
  taskbar = utilities.taskbar;

  # ------------ Waybar layouts ------------
  common = {
    layer = "top";
    height = 40;
    smooth-scrolling-threshold = 5;
  };

  # ------------ TopBar -------------

  topBar =
    common
    // {
      position = "top";
      expand-center = true;
      # Battery at the far left, and `custom/media` in the middle: one button
      # carrying Wayle's notification count and the current track. Wayle stays
      # hidden until this button opens its notification history.
      modules-left = [
        "custom/battery"
        "tray"
        "custom/ycal"
      ];
      modules-center = [
        "custom/media"
        "custom/lyrics"
      ];
      modules-right = [
        "custom/timer"
        "custom/clipboard"
        "custom/audio"
        "custom/network"
      ];
    }
    // modules;

  # ------------ BottomBar -------------

  starredApp = name: icon: cmd: {
    format = icon;
    tooltip = true;
    tooltip-format = name;
    on-click = "${cmd} &";
  };

  chromeApp =
    name: icon: appId:
    starredApp name icon "google-chrome-stable --profile-directory=Default --app-id=${appId}";

  bottomBar =
    common
    // {
      position = "bottom";
      modules-left = [
        "niri/workspaces"
        "custom/obsidian"
        "custom/gcal"
        "custom/gkeep"
        "custom/gphotos"
        "custom/whatsapp"
        "custom/tauon"
        "cffi/niri_window_buttons"
      ];

      # Workspace switcher: click to focus, middle-click to move up, right-click to move down
      "niri/workspaces" = {
        format = "{index}";
        tooltip-format = "Middle-click: move up  |  Right-click: move down";
        on-click-middle = "niri msg action focus-workspace {index} && niri msg action move-workspace-up";
        on-click-right = "niri msg action focus-workspace {index} && niri msg action move-workspace-down";
      };

      "custom/obsidian" = starredApp "Obsidian" "💎" "obsidian";
      "custom/tauon" = starredApp "Tauon" "🎵" "tauon";
      "custom/whatsapp" = starredApp "WhatsApp" "💬" "whatsie";
      "custom/gkeep" = chromeApp "Google Keep" "📝" "eilembjdkfgodjkcjnpgpaenohkicgjd";
      "custom/gcal" = chromeApp "Google Calendar" "📅" "kjbdgfilnfhdoflbpgamdcdgpehopbep";
      "custom/gphotos" = chromeApp "Google Photos" "🖼️" "ncmjhecbjeaamljdfahankockkkdmedg";
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
