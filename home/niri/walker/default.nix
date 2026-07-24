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
#   1. elephant's per-provider config fan-out. `services.elephant.settings`
#      only writes a single elephant/config.toml; the per-provider files
#      (elephant/clipboard.toml, elephant/menus/*.lua, ...) are generated
#      here from ./elephant.nix, which keeps its original shape.
#   2. `services.walker.theme` holds a single theme, so the two alternate
#      layouts are written out directly.
{
  pkgs,
  lib,
  ...
}:
let
  tomlFormat = pkgs.formats.toml { };

  # ── walker ────────────────────────────────────────────────
  # `theme` below sets settings.theme to the theme name, so it is
  # deliberately not repeated here.
  walkerSettings = {
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
        "files"
        "calc"
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
      # etc. do nothing. `settings` replaces walker's built-in default
      # config entirely, so we must re-declare the actions we want here.
      actions = import ./actions.nix;
    };
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
  base = builtins.readFile ../themes/walker-cyberpunk.css;
  layoutTopRight = builtins.readFile ../themes/walker-layout-top-right.xml;
  layoutTopCenter = builtins.readFile ../themes/walker-layout-top-center.xml;
  layoutTopLeft = builtins.readFile ../themes/walker-layout-top-left.xml;

  # `services.walker.theme` only models one theme, so the alternates are
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

  extraThemes =
    mkTheme "cyberpunk-center" {
      style = base;
      layouts = {
        "layout" = layoutTopCenter;
        "item_todo" = layoutTopCenter;
      };
    }
    // mkTheme "cyberpunk-left" {
      style = base;
      layouts."layout" = layoutTopLeft;
    };

  # ── elephant ──────────────────────────────────────────────
  # ./elephant.nix keeps the shape the upstream flake module expected
  # (provider.<name>.settings, provider.menus.lua/.toml); the fan-out
  # into individual config files is reproduced here.
  elephantCfg = import ./elephant.nix { inherit pkgs; };

  providerConfigs = lib.mapAttrs' (
    name: provider:
    lib.nameValuePair "elephant/${name}.toml" {
      source = tomlFormat.generate "${name}.toml" provider.settings;
    }
  ) (lib.filterAttrs (_: provider: provider ? settings) elephantCfg.provider);

  menuTomlFiles = lib.mapAttrs' (
    name: menu:
    lib.nameValuePair "elephant/menus/${name}.toml" {
      source = tomlFormat.generate "${name}.toml" menu;
    }
  ) (elephantCfg.provider.menus.toml or { });

  menuLuaFiles = lib.mapAttrs' (
    name: menu: lib.nameValuePair "elephant/menus/${name}.lua" { text = menu; }
  ) (elephantCfg.provider.menus.lua or { });

  # Restart the units when any of the generated config changes — the
  # upstream flake modules did this and walker/elephant only re-read
  # their config on start.
  restartTrigger = value: [ (builtins.hashString "sha256" (builtins.toJSON value)) ];
in
{
  services.elephant = {
    enable = true;
    # nixpkgs' elephant wraps its binary with ELEPHANT_PROVIDER_DIR
    # pointing at its own store path, so the provider .so files no
    # longer need to be linked into ~/.config/elephant/providers.
    #
    # The provider list is pinned to the set that was previously
    # installed. Building all of them would additionally pull in
    # aptpackages/archlinuxpkgs/dnfpackages (useless here, but they
    # would still show up in the providerlist) plus protonpass and
    # wireplumber.
    package = pkgs.elephant.override {
      enabledProviders = [
        "1password"
        "bitwarden"
        "bluetooth"
        "bookmarks"
        "calc"
        "clipboard"
        "desktopapplications"
        "files"
        "menus"
        "niriactions"
        "nirisessions"
        "playerctl"
        "providerlist"
        "runner"
        "snippets"
        "symbols"
        "todo"
        "unicode"
        "websearch"
        "windows"
      ];
    };
  };

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

  xdg.configFile = lib.mkMerge [
    extraThemes
    providerConfigs
    menuTomlFiles
    menuLuaFiles
  ];

  # Details the upstream home-manager modules leave out but the
  # previous setup relied on.
  systemd.user.services = {
    walker.Unit = {
      ConditionEnvironment = "WAYLAND_DISPLAY";
      PartOf = [ "graphical-session.target" ];
      X-Restart-Triggers = restartTrigger walkerSettings;
    };
    elephant = {
      Unit.ConditionEnvironment = "WAYLAND_DISPLAY";
      Service = {
        RestartSec = 1;
        # Clean up the socket on stop.
        ExecStopPost = "${pkgs.coreutils}/bin/rm -f /tmp/elephant.sock";
        X-Restart-Triggers = restartTrigger elephantCfg;
      };
    };
  };

  home.packages = with pkgs; [
    wtype # Wayland typing
    fd # files provider (elephant/walker)
  ];
}
