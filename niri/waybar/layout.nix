# Waybar layouts.
{
  pkgs,
  modules,
  taskbar,
}:
let
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
      modules-left = [
        "custom/ycal"
        "group/system"
        "group/hardware"
      ];
      modules-center = [
        "custom/media"
        "custom/lyrics"
      ];
      modules-right = [
        "tray"
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
        format = " {index} ";
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
  inherit topBar bottomBar;
}
