# Bottom waybar: niri workspace switcher (with per-window taskbar
# icons) + starred-app launcher buttons.
# Returns the bottomBar attrset (without bar outputs — merged by default.nix).
{ }:
{
  layer = "top";
  position = "bottom";
  height = 40;
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
  modules-center = [ ];
  modules-right = [ ];

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
    format = "{index} {name}";
    tooltip-format = "Workspace {index}: {name}";
  };

  # Per-window taskbar — shows text labels for windows on the current
  # workspace only. Click to focus, middle-click to close. Uses a local
  # Waybar patch (text-only) to render text instead of icons. Requires
  # Waybar master (PR #4997); see the waybar overlay in flake.nix.
  "niri/workspaces#taskbar" = {
    current-only = true;
    format-window-separator = " ";
    window-rewrite-default = "{app_id}";
    window-rewrite = {
      "app_id<dev.zed.Zed-Nightly>" = "zed";
      "app_id<tauonmb>" = "tauon";
      "app_id<com.ktechpit.whatsie>" = "whatsapp";
      "app_id<chrome-eilembjdkfgodjkcjnpgpaenohkicgjd-Default>" = "gkeep";
      "app_id<google-chrome>" = "chrome";
      "app_id<com.google.Chrome>" = "chrome";
      "app_id<org.gnome.Meld>" = "meld";
      "app_id<org.inkscape.Inkscape>" = "inkscape";
      "app_id<org.kde.kid3>" = "kid3";
      "app_id<org.pulseaudio.pavucontrol>" = "pavucontrol";
      "app_id<org.qbittorrent.qBittorrent>" = "qbittorrent";
      "app_id<org.shotcut.Shotcut>" = "shotcut";
      "app_id<com.obsproject.Studio>" = "obs";
      "app_id<dev.lizardbyte.app.Sunshine.*>" = "sunshine";
      "app_id<io.github.waylyrics.Waylyrics>" = "waylyrics";
      "app_id<io.github.qarmin.czkawka>" = "czkawka";
      "app_id<io.github.qarmin.krokiet>" = "krokiet";
      "app_id<io.github.JakubMelka.Pdf4qt.*>" = "pdf4qt";
      "app_id<com.github.qarmin.czkawka>" = "czkawka";
      "app_id<org.gnome.Screenshot>" = "screenshot";
      "app_id<OneDriveGUI>" = "onedrive";
      "app_id<wemeetapp>" = "wemeet";
      "app_id<startcenter>" = "libreoffice";
      "app_id<Handy>" = "handy";
    };
    workspace-taskbar = {
      enable = true;
      text-only = true;
    };
  };
}
