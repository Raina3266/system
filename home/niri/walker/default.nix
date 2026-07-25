# Walker launcher with cyberpunk theme.
#
# Replaces rofi — Walker is a GTK4 Wayland launcher that supports
# click-outside-to-close (unlike rofi on Wayland). Uses elephant's
# built-in clipboard/todo/files providers instead of custom
# shell scripts.
#
# Configured through home-manager's upstream `services.walker` /
# `services.elephant` modules and nixpkgs' walker/elephant packages,
# so no flake inputs are needed for either project. Two things the
# upstream modules don't cover are re-implemented below:
#
#   1. elephant's per-provider config fan-out — see ./elephant.nix.
#   2. `services.walker.theme` holds a single theme, so the two alternate
#      layouts are written out directly.
#   3. walker's own `providers` option (default/empty/prefixes/actions)
#      — see ./providers.nix, kept separate for readability.
{
  pkgs,
  lib,
  ...
}:
let
  # ── walker ────────────────────────────────────────────────
  # `theme` below sets settings.theme to the theme name, so it is
  # deliberately not repeated here.
  walkerSettings = {
    click_to_close = true;
    close_when_open = true;
    single_click_activation = true;
    # Hide the F1–F4 quick-activation buttons in the popup.
    hide_quick_activation = true;
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
    # See ./providers.nix: which providers are queried by default/on
    # empty query, prefix-triggered providers, and per-provider actions.
    providers = import ./providers.nix;
    # Global keybinds — also lost when `settings` replaces the default.
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

  # ── themes ────────────────────────────────────────────────
  base = builtins.readFile ../themes/walker.css;
  layoutTopRight = builtins.readFile ../themes/walker-top-right.xml;
  layoutTopCenter = builtins.readFile ../themes/walker-top-center.xml;

  # `services.walker.theme` only models one theme, so the alternate is
  # written out with the same file layout the module would produce.
  mkTheme =
    name:
    { style, layouts }:
    {
      "walker/themes/${name}/style.css".text = style;
    }
    // lib.mapAttrs' (
      layoutName: content: lib.nameValuePair "walker/themes/${name}/${layoutName}.xml" { text = content; }
    ) layouts;

  # Used by the waybar todo button (`walker -t cyberpunk-center -m todo`).
  extraThemes = mkTheme "cyberpunk-center" {
    style = base;
    layouts = {
      "layout" = layoutTopCenter;
      "item_todo" = layoutTopCenter;
    };
  };

  # Restart walker when its config changes — it only re-reads on start.
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
    # Default theme: top-right dropdown, sitting just under the top waybar.
    # The layout XML sets valign=start halign=end on the box-wrapper so it
    # actually anchors to the top-right (CSS margins alone can't override
    # GTK4 alignment properties set in the default layout).
    theme = {
      name = "cyberpunk";
      style = base;
      layout."layout" = layoutTopRight;
    };
  };

  xdg.configFile = extraThemes;

  # Details the upstream home-manager modules leave out but the
  # previous setup relied on.
  systemd.user.services.walker.Unit = {
    ConditionEnvironment = "WAYLAND_DISPLAY";
    PartOf = [ "graphical-session.target" ];
    X-Restart-Triggers = restartTrigger walkerSettings;
  };

  home.packages = with pkgs; [
    wtype # Wayland typing
  ];
}
