# niri_window_buttons: https://github.com/adelmonte/niri_window_buttons
# Taskbar (current workspace only): click=focus, middle=close, right=menu
# Drag to reorder, shift-click for multi-select
{ pkgs, packages }:
{
  homeConfig = {
    home.packages = [
      packages.previewPanel
      packages.rofiClipboard
    ];

    # The top bar is instantiated once per output. Keep these two stateful
    # processes independent of those copies and of Waybar restarts.
    systemd.user.services.waybar-timer = {
      Unit = {
        Description = "Shared Waybar countdown timer";
        ConditionEnvironment = [ "XDG_CURRENT_DESKTOP=niri" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${packages.waybarTimer}/bin/waybar-timer daemon";
        Restart = "on-failure";
        RestartSec = 2;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    systemd.user.services.rofi-clipboard-collector = {
      Unit = {
        Description = "Capture Wayland clipboard history once per session";
        ConditionEnvironment = [ "WAYLAND_DISPLAY" "XDG_CURRENT_DESKTOP=niri" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${pkgs.wl-clipboard}/bin/wl-paste --watch ${packages.rofiClipboard}/bin/rofi-clipboard capture";
        Restart = "always";
        RestartSec = 2;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };

  modules = {
    "custom/timer" = {
      exec = "${packages.waybarTimer}/bin/waybar-timer status";
      interval = 1;
      format = "{}";
      return-type = "json";
      tooltip = true;
      escape = false;
      on-click = "${packages.waybarTimer}/bin/waybar-timer add";
      on-click-middle = "${packages.waybarTimer}/bin/waybar-timer toggle";
      on-click-right = "${packages.waybarTimer}/bin/waybar-timer clear";
    };

    "custom/clipboard" = {
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.rofiClipboard}/bin/rofi-clipboard status";
      return-type = "json";
      tooltip = true;
      escape = false;
      "restart-interval" = 1;
      "exec-on-event" = false;
      on-click = "${packages.rofiClipboard}/bin/rofi-clipboard";
      on-click-right = "${packages.rofiClipboard}/bin/rofi-clipboard clear";
    };

    tray = {
      icon-size = 18;
      spacing = 10;
    };
  };

  taskbar = {
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
          label = " Maximize to Edges ";
          action = "maximize-window-to-edges";
        }
        {
          label = " Center Column ";
          action = "center-column";
        }
        {
          label = " Toggle Floating ";
          action = "toggle-window-floating";
        }
        {
          label = " Move Up ";
          action = "move-window-to-workspace-up";
        }
        {
          label = " Move Down ";
          action = "move-window-to-workspace-down";
        }
        {
          label = " Close Window ";
          action = "close-window";
        }
      ];

      # Multi-select: Shift+click windows, right-click for batch actions
      multi_select_modifier = "shift";
      multi_select_menu = [
        {
          label = " Move All Up ";
          action = "move-to-workspace-up";
        }
        {
          label = " Move All Down ";
          action = "move-to-workspace-down";
        }
        {
          label = " Maximize All ";
          action = "maximize-columns";
        }
        {
          label = " Close All ";
          action = "close-windows";
        }
      ];

      # Urgency hints when app requests attention
      notifications = {
        enabled = true;
        use_desktop_entry = true;
        use_fuzzy_matching = true;
      };
    };
  };
}
