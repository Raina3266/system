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

  topBar = common // modules // {
    position = "top";
    expand-center = true;
    modules-left = [ "custom/ycal" "tray" ];
    modules-center = [ "custom/media" "custom/lyrics" ];
    modules-right = [
      "custom/timer"
      "custom/clipboard-panel"
      "custom/audio"
      "custom/network"
      "custom/dashboard"
    ];
  };

  panelOutputs = [
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
        "custom/media" = system.mediaModule output;
        "custom/clipboard-panel" = system.clipboardModule output;
      };
    }) panelOutputs
  );

  # ------------ BottomBar -------------

  # No `output`: one config shown on every monitor. The taskbar sizes itself
  # per monitor through max_taskbar_width_per_output.
  bottomBar = common // taskbar // {
    position = "bottom";
    modules-left = [
      "niri/workspaces"
      "custom/obsidian"
      "custom/gcal"
      "custom/gkeep"
      "custom/gphotos"
      "custom/whatsapp"
      "custom/anki"
      "cffi/niri_window_buttons"
    ];
    "niri/workspaces" = {
      format = "{index}";
      tooltip-format = "Middle-click: move up  |  Right-click: move down";
      on-click-middle = "niri msg action focus-workspace {index} && niri msg action move-workspace-up";
      on-click-right = "niri msg action focus-workspace {index} && niri msg action move-workspace-down";
    };
    "custom/obsidian" = {
      format = "💎";
      tooltip = true;
      tooltip-format = "Obsidian";
      on-click = "obsidian &";
    };
    "custom/anki" = {
      format = "🧠";
      tooltip = true;
      tooltip-format = "Anki";
      on-click = "anki &";
    };
    "custom/whatsapp" = {
      format = "💬";
      tooltip = true;
      tooltip-format = "WhatsApp";
      on-click = "whatsie &";
    };
    "custom/gkeep" = {
      format = "📝";
      tooltip = true;
      tooltip-format = "Google Keep";
      on-click = "google-chrome-stable --profile-directory=Default --app-id=eilembjdkfgodjkcjnpgpaenohkicgjd &";
    };
    "custom/gcal" = {
      format = "📅";
      tooltip = true;
      tooltip-format = "Google Calendar";
      on-click = "google-chrome-stable --profile-directory=Default --app-id=kjbdgfilnfhdoflbpgamdcdgpehopbep &";
    };
    "custom/gphotos" = {
      format = "🖼️";
      tooltip = true;
      tooltip-format = "Google Photos";
      on-click = "google-chrome-stable --profile-directory=Default --app-id=ncmjhecbjeaamljdfahankockkkdmedg &";
    };
  };
in
{
  options.programs'.waybar.enable = lib.mkEnableOption "waybar";

  config = lib.mkIf (pkgs.stdenv.hostPlatform.isLinux && cfg.enable) (
    lib.mkMerge [
      system.homeConfig
      utilities.homeConfig

      {
        xdg.configFile."waybar/style.css".source =
          config.lib.file.mkOutOfStoreSymlink "${repoRoot}/niri/waybar/waybar.css";

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
          settings = topBars // {
            inherit bottomBar;
          };
        };

        systemd.user.services.waybar = {
          Unit = {
            ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
            Wants = [
              "waybar-timer.service"
              "clipboard-collector.service"
              "ags-media.service"
              "audio-control-ui.service"
              "ags-dashboard.service"
              "ags-network.service"
              "ags-app-launcher.service"
              "ags-clipboard.service"
            ];
            After = [
              "waybar-timer.service"
              "clipboard-collector.service"
              "ags-media.service"
              "audio-control-ui.service"
              "ags-dashboard.service"
              "ags-network.service"
              "ags-app-launcher.service"
              "ags-clipboard.service"
            ];
          };
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };

      })
    ]
  );
}
