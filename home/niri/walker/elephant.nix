# Elephant provider configuration and home-manager wiring.
#
# Owns everything elephant-related that default.nix used to inline:
#   - services.elephant (package + enabled providers)
#   - per-provider config fan-out into xdg.configFile
#     (provider.<name>.settings → elephant/<name>.toml,
#      provider.menus.lua/*    → elephant/menus/*.lua,
#      provider.menus.toml/*   → elephant/menus/*.toml)
#   - systemd.user.services.elephant (incl. restart trigger)
#   - home.packages that elephant providers depend on
{
  pkgs,
  lib,
  ...
}:
let
  tomlFormat = pkgs.formats.toml { };

  cfg = {
    # Keep clipboard history tidy: prune entries older than 3 days
    # (4320 minutes), matching the previous cliphist behaviour.
    provider.clipboard.settings = {
      auto_cleanup = 4320;
      max_items = 1000;
      pinned_on_top = true;
    };
    # Todo provider — task list stored in ~/.cache/elephant/todo.csv.
    # These are the defaults; declared explicitly so they're documented
    # here and can be tweaked. The popup only lists entries when tasks
    # exist — when empty, type a name and press Return to create one.
    provider.todo.settings = {
      # Minutes before/after a scheduled time during which a task is
      # shown as urgent (red). Notifications fire at the scheduled time.
      urgent_time_frame = 30;
      # Lower other players' volume while a task notification plays.
      duck_player_volumes = true;
      # Show creation time in the subtext when no other time info exists.
      show_creation_time = false;
      # Time format for subtext (Go time layout).
      time_format = "01-02 15:04";
      # Categories: prefix a query with `prefix` to file the new task
      # under that category (e.g. `w:fix report`). Cycling an existing
      # task's category is bound to ctrl+y (change_category).
      categories = [
        {
          name = "work";
          prefix = "w:";
        }
        {
          name = "personal";
          prefix = "p:";
        }
      ];
      # Notification shown when a scheduled task's time arrives.
      # %TASK% is replaced with the task's text. (These fields are
      # squashed into the top level of the todo settings upstream, not
      # nested under a "notification" key.)
      title = "Task Due";
      body = "🔔 %TASK%";
    };
    # Audio menu — see ./audio.nix.
    provider.menus.lua."audio" = import ./audio.nix { inherit pkgs; };
    provider.menus.toml."power" = {
      name = "power";
      name_pretty = "Power";
      icon = "system-shutdown";
      action = "sh -c '%VALUE%'";
      entries = [
        {
          text = "   Shutdown";
          value = "systemctl poweroff";
        }
        {
          text = "   Reboot";
          value = "systemctl reboot";
        }
        {
          text = "   Suspend";
          value = "systemctl suspend";
        }
        {
          text = "   Logout";
          value = "niri msg action quit";
        }
      ];
    };
    # Bluetooth menu — D-Bus (gdbus) based; replaces elephant's built-in
    # bluetooth provider, whose bluetoothctl one-shot "pair" races the
    # persistent bt-agent and hangs at "Pairing...". See bluetooth.nix.
    # Invoked via `walker -m menus:bluetooth`.
    provider.menus.lua."bluetooth" = (import ./bluetooth.nix { inherit pkgs; }).menuLua;

    # Wi-Fi menu — see ./wifi.nix. Invoked via `walker -m menus:wifi`.
    provider.menus.lua."wifi" = import ./wifi.nix { inherit pkgs; };
  };

  # ── config fan-out ────────────────────────────────────────
  # `services.elephant.settings` only writes a single elephant/config.toml;
  # the per-provider files are generated here from the cfg attrset above.
  providerConfigs = lib.mapAttrs' (
    name: provider:
    lib.nameValuePair "elephant/${name}.toml" {
      source = tomlFormat.generate "${name}.toml" provider.settings;
    }
  ) (lib.filterAttrs (_: provider: provider ? settings) cfg.provider);

  menuTomlFiles = lib.mapAttrs' (
    name: menu:
    lib.nameValuePair "elephant/menus/${name}.toml" {
      source = tomlFormat.generate "${name}.toml" menu;
    }
  ) (cfg.provider.menus.toml or { });

  menuLuaFiles = lib.mapAttrs' (
    name: menu: lib.nameValuePair "elephant/menus/${name}.lua" { text = menu; }
  ) (cfg.provider.menus.lua or { });

  # Restart elephant when any provider config changes — elephant only
  # re-reads its config on start.
  restartTrigger = [ (builtins.hashString "sha256" (builtins.toJSON cfg)) ];
in
{
  services.elephant = {
    enable = true;
    # nixpkgs' elephant wraps its binary with ELEPHANT_PROVIDER_DIR
    # pointing at its own store path, so the provider .so files no
    # longer need to be linked into ~/.config/elephant/providers.
    #
    # The provider list is pinned: building all of them would pull in
    # aptpackages/archlinuxpkgs/dnfpackages (useless here, but they
    # would still show up in the providerlist) plus protonpass.
    #
    # wireplumber needs `pw-dump` and `wpctl` on elephant's PATH or it
    # disables itself at load; both come from pkgs.wireplumber, added
    # to home.packages below.
    package = pkgs.elephant.override {
      enabledProviders = [
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
        "wireplumber"
      ];
    };
  };

  xdg.configFile = lib.mkMerge [
    providerConfigs
    menuTomlFiles
    menuLuaFiles
  ];

  systemd.user.services.elephant = {
    Unit.ConditionEnvironment = "WAYLAND_DISPLAY";
    Service = {
      RestartSec = 1;
      # Clean up the socket on stop.
      ExecStopPost = "${pkgs.coreutils}/bin/rm -f /tmp/elephant.sock";
      X-Restart-Triggers = restartTrigger;
    };
  };

  home.packages = with pkgs; [
    fd # files provider
    wireplumber # wpctl/pw-dump — wireplumber provider + audio menu
  ];
}
