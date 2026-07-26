# Bottom bar: niri workspaces + taskbar (niri_window_buttons CFFI) + app launchers
{ pkgs }:
let
  # niri_window_buttons: Waybar CFFI taskbar module for niri compositor
  # https://github.com/adelmonte/niri_window_buttons
  niri-window-buttons = pkgs.rustPlatform.buildRustPackage rec {
    pname = "niri_window_buttons";
    version = "0.4.3";

    src = pkgs.fetchFromGitHub {
      owner = "adelmonte";
      repo = "niri_window_buttons";
      tag = "v${version}";
      hash = "sha256-CUeeDe5DY7IRf6pCl9g7q5rHNs4ca4mAg0eKgZ0ErlY=";
    };

    cargoHash = "sha256-STrFRNLgytpLilx0o/StCAnaO1dyWDUQDoTzb7PA2hc=";

    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = with pkgs; [
      glib
      gtk3
      cairo
      pango
      gdk-pixbuf
      atk
      libpulseaudio
    ];

    doCheck = false;

    meta = {
      description = "Waybar CFFI module for traditional window buttons in the niri compositor";
      homepage = "https://github.com/adelmonte/niri_window_buttons";
      license = pkgs.lib.licenses.gpl3Plus;
      platforms = pkgs.lib.platforms.linux;
    };
  };

  # App launcher button: emoji icon + tooltip
  starredApp = name: icon: cmd: {
    format = icon;
    tooltip = true;
    tooltip-format = name;
    on-click = "${cmd} &";
  };

  # Chrome PWA launcher
  chromeApp = name: icon: appId: starredApp name icon "google-chrome-stable --profile-directory=Default --app-id=${appId}";
in
{
  layer = "top";
  position = "bottom";
  height = 40;
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
    "cffi/niri_window_buttons"
  ];

  "custom/yazi" = starredApp "Yazi" "🐤" "ghostty -e yazi";
  "custom/thunderbird" = starredApp "Thunderbird" "📧" "thunderbird";
  "custom/obsidian" = starredApp "Obsidian" "💎" "obsidian";
  "custom/tauon" = starredApp "Tauon" "🎵" "tauon";
  "custom/whatsapp" = starredApp "WhatsApp" "💬" "whatsie";
  "custom/gkeep" = chromeApp "Google Keep" "📝" "eilembjdkfgodjkcjnpgpaenohkicgjd";
  "custom/gcal" = chromeApp "Google Calendar" "📅" "kjbdgfilnfhdoflbpgamdcdgpehopbep";
  "custom/gphotos" = chromeApp "Google Photos" "🖼️" "ncmjhecbjeaamljdfahankockkkdmedg";

  # Workspace switcher: click to focus, middle-click to move up, right-click to move down
  "niri/workspaces" = {
    format = " {index} ";
    tooltip-format = "Middle-click: move up  |  Right-click: move down";
    on-click-middle = "niri msg action focus-workspace {index} && niri msg action move-workspace-up";
    on-click-right = "niri msg action focus-workspace {index} && niri msg action move-workspace-down";
  };

  # Taskbar (current workspace only): click=focus, middle=close, right=menu
  # Drag to reorder, shift-click for multi-select
  "cffi/niri_window_buttons" = {
    module_path = "${niri-window-buttons}/lib/libniri_window_buttons.so";

    only_current_workspace = true;
    show_window_titles = true;
    truncate_titles = true;
    show_tooltip = true;

    icon_size = 22;
    icon_spacing = 10;
    min_button_width = 120;
    max_button_width = 200;
    # Default to eDP-1 width (overridden per-output below)
    max_taskbar_width = 1300;
    scroll_arrow_left = " ◀ ";
    scroll_arrow_right = " ▶ ";

    # Per-monitor logical widths (mode width / scale, see niri/config.kdl)
    max_taskbar_width_per_output = {
      "eDP-1" = 1350; # 1920x1200 @ 1x
      "DP-8" = 2000; # 2560x1440 @ 1x
      "DP-7" = 1700; # 2560x2880 @ 1.25x = 2048 logical
    };

    # Button size mirrors window width (clamped to min/max)
    proportional_button_width = true;
    proportional_icon_size = true;

    # Drag reorder: browser-style (button follows cursor)
    drag_style = "browser";
    drag_hover_focus = true;
    drag_hover_focus_delay = 500;

    click_actions = {
      left_click_unfocused = "focus-window";
      left_click_focused = "focus-window";
      middle_click_unfocused = "close-window";
      middle_click_focused = "close-window";
      right_click_unfocused = "menu";
      right_click_focused = "menu";
    };

    context_menu = [
      { label = "  Maximize Column"; action = "maximize-column"; }
      { label = "  Maximize to Edges"; action = "maximize-window-to-edges"; }
      { label = "  Center Column"; action = "center-column"; }
      { label = "󰉩  Toggle Floating"; action = "toggle-window-floating"; }
      { label = "  Move WS Up"; action = "move-window-to-workspace-up"; }
      { label = "  Move WS Down"; action = "move-window-to-workspace-down"; }
      { label = "  Close Window"; action = "close-window"; }
    ];

    # Multi-select: Shift+click windows, right-click for batch actions
    multi_select_modifier = "shift";
    multi_select_menu = [
      { label = "  Move All Up"; action = "move-to-workspace-up"; }
      { label = "  Move All Down"; action = "move-to-workspace-down"; }
      { label = "  Maximize All"; action = "maximize-columns"; }
      { label = "  Close All"; action = "close-windows"; }
    ];

    # Audio indicator disabled: libpulse glib-mainloop double-free crashes waybar
    # audio_indicator = {
    #   enabled = true;
    #   playing_icon = "󰕾 ";
    #   muted_icon = "󰖁 ";
    #   clickable = true;
    # };

    # Urgency hints when app requests attention
    notifications = {
      enabled = true;
      use_desktop_entry = true;
      use_fuzzy_matching = true;
    };
  };
}