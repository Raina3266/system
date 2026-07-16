# Bottom waybar: niri workspace switcher (with per-window taskbar
# icons) + starred-app launcher buttons.
# Returns the bottomBar attrset (without bar outputs — merged by default.nix).
{ }:
{
  layer = "top";
  position = "bottom";
  height = 42;
  smooth-scrolling-threshold = 5;

  modules-left = [
    "niri/workspaces"
    "custom/thunar"
    "custom/thunderbird"
    "custom/gcal"
    "custom/obsidian"
    "custom/tauon"
    "custom/whatsapp"
    "custom/gkeep"
    "custom/gphotos"
    "niri/workspaces#taskbar"
  ];

  "custom/thunar" = {
    format = "📁";
    tooltip = true;
    tooltip-format = "Thunar";
    on-click = "thunar &";
  };
  "custom/thunderbird" = {
    format = "📧";
    tooltip = true;
    tooltip-format = "Thunderbird";
    on-click = "thunderbird &";
  };
  "custom/obsidian" = {
    format = "📝";
    tooltip = true;
    tooltip-format = "Obsidian";
    on-click = "obsidian &";
  };
  "custom/tauon" = {
    format = "🎵";
    tooltip = true;
    tooltip-format = "Tauon";
    on-click = "tauon &";
  };
  "custom/whatsapp" = {
    format = "💬";
    tooltip = true;
    tooltip-format = "WhatsApp";
    on-click = "whatsie &";
  };
  "custom/gkeep" = {
    format = "🗒️";
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
