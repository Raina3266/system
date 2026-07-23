# Walker launcher with cyberpunk theme.
#
# Replaces rofi — Walker is a GTK4 Wayland launcher that supports
# click-outside-to-close (unlike rofi on Wayland). Uses elephant's
# built-in clipboard/todo/files providers instead of custom
# shell scripts.
{ pkgs, inputs, ... }:
{
  imports = [ inputs.walker.homeManagerModules.default ];

  programs.walker = {
    enable = true;
    runAsService = true;
    config = {
      theme = "cyberpunk";
      click_to_close = true;
      close_when_open = true;
      single_click_activation = true;
      # Hide the F1–F4 quick-activation buttons in the popup.
      hide_quick_activation = true;
      preview.content_fit = "contain";
      # Layer-shell anchoring: keep the default fullscreen overlay (all
      # four anchors) so click-to-close works — clicks on the empty area
      # around the box-wrapper dismiss walker. The box-wrapper itself is
      # nudged to the top-right corner via CSS margins in the theme.
      shell = {
        exclusive_zone = -1;
        layer = "overlay";
        anchor_top = true;
        anchor_bottom = true;
        anchor_left = true;
        anchor_right = true;
      };
      placeholders = {
        "default".input = "Search";
        "default".list = "No Results";
        clipboard.input = "Clipboard";
        clipboard.list = "Clipboard is empty";
        todo.input = "Add or search a task…";
        todo.list = "No tasks";
        windows.input = "Search windows…";
        windows.list = "No open windows";
        files.input = "Search files…";
        files.list = "No files found";
        symbols.input = "Search symbols…";
        symbols.list = "No symbols found";
        unicode.input = "Search unicode…";
        unicode.list = "No characters found";
        runner.input = "Run command…";
        runner.list = "No matching commands";
        playerctl.input = "Search players…";
        playerctl.list = "No media players";
        wireplumber.input = "Search audio devices…";
        wireplumber.list = "No audio devices";
        nirisessions.input = "Search sessions…";
        nirisessions.list = "No sessions defined";
      };
      providers = {
        # Providers queried by default (empty query) and on launch.
        # Without these, walker shows nothing when opened plain.
        default = [
          "desktopapplications"
          "clipboard"
          "files"
          "todo"
          "menus"
        ];
        empty = [ "desktopapplications" ];
        prefixes = [
          {
            provider = "clipboard";
            prefix = ":";
          }
          {
            provider = "todo";
            prefix = "!";
          }
          {
            provider = "providerlist";
            prefix = ";";
          }
          {
            provider = "websearch";
            prefix = "@";
          }
          {
            provider = "calc";
            prefix = "=";
          }
          {
            provider = "symbols";
            prefix = ".";
          }
          {
            provider = "unicode";
            prefix = "u:";
          }
          {
            provider = "files";
            prefix = "/";
          }
          {
            provider = "windows";
            prefix = "$";
          }
          {
            provider = "runner";
            prefix = ">";
          }
          {
            provider = "playerctl";
            prefix = "mp:";
          }
          {
            provider = "wireplumber";
            prefix = "au:";
          }
          {
            provider = "nirisessions";
            prefix = "ses:";
          }
        ];
        # Per-provider action keybinds. These are REQUIRED for the todo
        # provider (and others) to function — without them, Enter/Ctrl+D
        # etc. do nothing. The walker module's `config` option replaces
        # its default (imported from resources/config.toml) entirely, so
        # we must re-declare the actions we want here.
        actions = import ./actions.nix;
      };
      # Global keybinds — also lost when `config` replaces the default.
      keybinds = {
        close = [ "Escape" ];
        next = [ "Down" ];
        previous = [ "Up" ];
        left = [ "Left" ];
        right = [ "Right" ];
        down = [ "Down" ];
        up = [ "Up" ];
        toggle_exact = [ "ctrl e" ];
        resume_last_query = [ "ctrl r" ];
        page_down = [ "Page_Down" ];
        page_up = [ "Page_Up" ];
        show_actions = [ "alt j" ];
      };
    };
    # Elephant provider configuration (merged into elephant's config).
    elephant = import ./elephant.nix { inherit pkgs; };
    themes =
      let
        base = builtins.readFile ../themes/walker-cyberpunk.css;
        layoutTopRight = builtins.readFile ../themes/walker-layout-top-right.xml;
        layoutTopCenter = builtins.readFile ../themes/walker-layout-top-center.xml;
        layoutTopLeft = builtins.readFile ../themes/walker-layout-top-left.xml;
        # item_todo.xml override: shrinks the hardcoded 48px "+" create-entry
        # icon to 16px. Applies to both themes.
        itemTodo = builtins.readFile ../themes/walker-item-todo.xml;
      in
      {
        # Default theme: top-right dropdown, sitting just under the top waybar.
        # The layout XML sets valign=start halign=end on the box-wrapper so it
        # actually anchors to the top-right (CSS margins alone can't override
        # GTK4 alignment properties set in the default layout).
        cyberpunk = {
          style = base;
          layouts."layout" = layoutTopRight;
          layouts."item_todo" = itemTodo;
        };
        # Top-center variant, used by popups launched with `-t cyberpunk-center`
        # (currently just the waybar todo module). Same styling; only the
        # box-wrapper alignment differs (halign=center).
        cyberpunk-center = {
          style = base;
          layouts."layout" = layoutTopCenter;
          layouts."item_todo" = itemTodo;
        };
        # Top-left variant, for popups that should anchor to the top-left
        # corner (e.g. waybar-ycal). Same styling; only the box-wrapper
        # alignment differs (halign=start).
        cyberpunk-left = {
          style = base;
          layouts."layout" = layoutTopLeft;
          layouts."item_todo" = itemTodo;
        };
      };
  };

  home.packages = with pkgs; [
    wtype # Wayland typing
    fd # files provider (elephant/walker)
  ];
}
