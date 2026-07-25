# Walker: GTK4 Wayland launcher with cyberpunk theme.
# Replaces rofi, supports click-outside-to-close and elephant providers
# (clipboard/todo/files).
#
# Extensions beyond upstream home-manager modules:
#   1. elephant per-provider config (see ./elephant.nix)
#   2. alternate theme layouts written directly (services.walker.theme
#      only supports one theme)
#   3. providers config (default/empty/prefixes/actions in ./providers.nix)
{
  pkgs,
  lib,
  ...
}:
let
  # Walker settings (theme name set via `theme` attribute below)
  walkerSettings = {
    click_to_close = true;
    close_when_open = true;
    single_click_activation = true;
    # Hide F1-F4 quick-activation buttons
    hide_quick_activation = true;
    # Layer-shell: fullscreen overlay (all anchors) for click-to-close.
    # Box-wrapper positioned via CSS margins in theme.
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
    # Provider config in ./providers.nix
    providers = import ./providers.nix;
    # Global keybinds
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

  # Theme files
  base = builtins.readFile ../themes/walker.css;
  layoutTopRight = builtins.readFile ../themes/walker-top-right.xml;
  layoutTopCenter = builtins.readFile ../themes/walker-top-center.xml;

  # services.walker.theme supports one theme; alternate written manually
  mkTheme =
    name:
    { style, layouts }:
    {
      "walker/themes/${name}/style.css".text = style;
    }
    // lib.mapAttrs' (
      layoutName: content: lib.nameValuePair "walker/themes/${name}/${layoutName}.xml" { text = content; }
    ) layouts;

  # Alternate theme for waybar todo button (center-positioned)
  extraThemes = mkTheme "cyberpunk-center" {
    style = base;
    layouts = {
      "layout" = layoutTopCenter;
      "item_todo" = layoutTopCenter;
    };
  };

  # Restart walker on config changes (only reads config at startup)
  restartTrigger = value: [ (builtins.hashString "sha256" (builtins.toJSON value)) ];
in
{
  imports = [ ./elephant.nix ];

  services.walker = {
    enable = true;
    package = pkgs.walker;
    systemd.enable = true;
    enableElephantIntegration = true;
    settings = walkerSettings;
    # Default theme: top-right dropdown below waybar.
    # Layout XML sets valign=start halign=end on box-wrapper for positioning.
    theme = {
      name = "cyberpunk";
      style = base;
      layout."layout" = layoutTopRight;
    };
  };

  xdg.configFile = extraThemes;

  # Additional systemd service config not in upstream modules
  systemd.user.services.walker.Unit = {
    ConditionEnvironment = "WAYLAND_DISPLAY";
    PartOf = [ "graphical-session.target" ];
    X-Restart-Triggers = restartTrigger walkerSettings;
  };

  home.packages = with pkgs; [
    wtype # Wayland text input simulation
  ];
}
