# Bottom waybar: niri workspace switcher (with per-window taskbar
# icons) + starred-app launcher buttons.
# Returns the bottomBar attrset (without bar outputs — merged by default.nix).
{ }:
let
  # Starred-app launcher button: emoji icon + tooltip + background launch.
  starredApp = name: icon: cmd: {
    format = icon;
    tooltip = true;
    tooltip-format = name;
    on-click = "${cmd} &";
  };

  # Google Chrome PWA launcher by app id.
  chromeApp = name: icon: appId: starredApp name icon "google-chrome-stable --profile-directory=Default --app-id=${appId}";
in
{
  layer = "top";
  position = "bottom";
  height = 42;
  smooth-scrolling-threshold = 5;

  modules-left = [
    "niri/workspaces"
    "custom/thunderbird"
    "custom/gcal"
    "custom/gkeep"
    "custom/gphotos"
    "custom/obsidian"
    "custom/tauon"
    "custom/whatsapp"
    "custom/yazi"
    "niri/workspaces#taskbar"
  ];

  "custom/yazi" = starredApp "Yazi" "🐤" "ghostty -e yazi";
  "custom/thunderbird" = starredApp "Thunderbird" "📧" "thunderbird";
  "custom/obsidian" = starredApp "Obsidian" "💎" "obsidian";
  "custom/tauon" = starredApp "Tauon" "🎙️" "tauon";
  "custom/whatsapp" = starredApp "WhatsApp" "💬" "whatsie";
  "custom/gkeep" = chromeApp "Google Keep" "📝" "eilembjdkfgodjkcjnpgpaenohkicgjd";
  "custom/gcal" = chromeApp "Google Calendar" "📅" "kjbdgfilnfhdoflbpgamdcdgpehopbep";
  "custom/gphotos" = chromeApp "Google Photos" "🖼️" "ncmjhecbjeaamljdfahankockkkdmedg";

  # Workspace switcher — shows index + name for each workspace.
  "niri/workspaces" = {
    format = " {index} ";
    tooltip-format = "Workspace";
  };

  # Per-window taskbar — shows app icon + text title for windows on
  # the current workspace only. Click to focus, middle-click to close.
  "niri/workspaces#taskbar" = {
    expand = true;
    current-only = true;
    hide-empty = true;
    format-window-separator = "  ";
    window-rewrite-default = "";
    workspace-taskbar = {
      enable = true;
      icon-size = 22;
    };
  };
}
