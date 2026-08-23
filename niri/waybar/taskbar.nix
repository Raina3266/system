# niri_window_buttons: Waybar CFFI taskbar module for niri compositor
# https://github.com/adelmonte/niri_window_buttons
# Taskbar (current workspace only): click=focus, middle=close, right=menu
# Drag to reorder, shift-click for multi-select
{ packages }:
{
  "cffi/niri_window_buttons" = {
    module_path = "${packages.niriWindowButtons}/lib/libniri_window_buttons.so";

    only_current_workspace = true;
    show_window_titles = true;
    truncate_titles = true;
    show_tooltip = true;

    icon_size = 25;
    icon_spacing = 5;
    min_button_width = 120;
    max_button_width = 200;
    # Default to eDP-1 width (overridden per-output below)
    max_taskbar_width = 1400;
    scroll_arrow_left = "◀";
    scroll_arrow_right = "▶";

    # Per-monitor logical widths (mode width / scale, see niri/config.kdl)
    max_taskbar_width_per_output = {
      "eDP-1" = 1400; # 1920x1200 @ 1x
      "DP-8" = 2000; # 2560x1440 @ 1x
      "DP-7" = 1700; # 2560x2880 @ 1.25x = 2048 logical
    };

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
      {
        label = "  Maximize to Edges  ";
        action = "maximize-window-to-edges";
      }
      {
        label = "  Center Column  ";
        action = "center-column";
      }
      {
        label = "  Toggle Floating  ";
        action = "toggle-window-floating";
      }
      {
        label = "  Move Up  ";
        action = "move-window-to-workspace-up";
      }
      {
        label = "  Move Down  ";
        action = "move-window-to-workspace-down";
      }
      {
        label = "  Close Window  ";
        action = "close-window";
      }
    ];

    # Multi-select: Shift+click windows, right-click for batch actions
    multi_select_modifier = "shift";
    multi_select_menu = [
      {
        label = "  Move All Up  ";
        action = "move-to-workspace-up";
      }
      {
        label = "  Move All Down  ";
        action = "move-to-workspace-down";
      }
      {
        label = "  Maximize All  ";
        action = "maximize-columns";
      }
      {
        label = "  Close All  ";
        action = "close-windows";
      }
    ];

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
