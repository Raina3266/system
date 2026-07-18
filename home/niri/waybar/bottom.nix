# Bottom waybar: niri workspace switcher + niri_window_buttons taskbar
# (per-window icon/title buttons via the niri_window_buttons CFFI module)
# + starred-app launcher buttons.
# Returns the bottomBar attrset (without bar outputs — merged by default.nix).
{ pkgs }:
let
  # Standalone Waybar CFFI module providing rich per-window taskbar buttons
  # for the niri compositor: https://github.com/adelmonte/niri_window_buttons
  #
  # Produces $out/lib/libniri_window_buttons.so, referenced by module_path in
  # the cffi/niri_window_buttons config below.
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
  height = 40;
  smooth-scrolling-threshold = 5;

  modules-center = [
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

  # Workspace switcher — shows index + name for each workspace.
  # Middle-click moves the clicked workspace up; right-click moves it down.
  # (niri has no close-workspace action, so middle-click reorders instead.)
  # The workspace is focused first so niri's move acts on the right one.
  "niri/workspaces" = {
    format = " {index} ";
    tooltip-format = "Middle-click: move up  |  Right-click: move down";
    on-click-middle = "niri msg action focus-workspace {index} && niri msg action move-workspace-up";
    on-click-right = "niri msg action focus-workspace {index} && niri msg action move-workspace-down";
  };

  # Per-window taskbar — shows app icon + title for windows on the current
  # workspace only. Click to focus, middle-click to close, right-click for
  # a context menu. Drag to reorder, shift-click to multi-select. Buttons
  # shrink down to min_button_width before the taskbar starts scrolling.
  "cffi/niri_window_buttons" = {
    module_path = "${niri-window-buttons}/lib/libniri_window_buttons.so";

    only_current_workspace = true;
    show_window_titles = true;
    truncate_titles = true;
    show_tooltip = true;

    icon_size = 25;
    icon_spacing = 8;
    min_button_width = 100;
    max_button_width = 220;
    # Fall back to the eDP-1 logical width; overridden per-output below to
    # match each display's actual logical resolution (see niri/config.kdl).
    max_taskbar_width = 1350;
    scroll_arrow_left = " ◀ ";
    scroll_arrow_right = " ▶ ";

    # Let the taskbar grow to fill each monitor's full logical width instead
    # of a fixed pixel count. Logical width = mode width / scale (see
    # niri/config.kdl for each output's mode + scale).
    max_taskbar_width_per_output = {
      "eDP-1" = 1350; # 1920x1200 @ scale 1
      "DP-8" = 2000; # 2560x1440 @ scale 1
      "DP-7" = 1700; # 2560x2880 @ scale 1.25 -> 2048 logical
    };

    # Size each button to mirror its window's on-screen width in the niri
    # layout, clamped between min_button_width and max_button_width.
    proportional_button_width = true;
    proportional_icon_size = true;

    # Drag reorder — browser-style: grabbed button follows the cursor while
    # neighbors slide around it.
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

    # Multi-select: hold Shift + left-click to select several windows,
    # then right-click for batch actions.
    multi_select_modifier = "shift";
    multi_select_menu = [
      { label = "  Move All Up"; action = "move-to-workspace-up"; }
      { label = "  Move All Down"; action = "move-to-workspace-down"; }
      { label = "  Maximize All"; action = "maximize-columns"; }
      { label = "  Close All"; action = "close-windows"; }
    ];

    # Speaker icon on windows currently playing audio; click to mute.
    # Disabled: libpulse's glib-mainloop integration has a known
    # double-free bug in its timer teardown ("glib_time_free: assndicator = {
    audio_indicator = {
      enabled = true;
      playing_icon = "󰕾";
      muted_icon = "󰖁";
      clickable = true;
    };

    # Urgency-hint highlighting when an app requests attention.
    notifications = {
      enabled = true;
      use_desktop_entry = true;
      use_fuzzy_matching = true;
    };
  };
}