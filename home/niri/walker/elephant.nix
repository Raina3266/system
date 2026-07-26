# Elephant: provider configuration for Walker launcher.
# Manages:
#   - services.elephant (package + enabled providers)
#   - per-provider config files (settings → elephant/<name>.toml,
#     lua menus → elephant/menus/*.lua, toml menus → elephant/menus/*.toml)
#   - systemd.user.services.elephant (with restart trigger)
#   - provider dependencies (fd, wireplumber)
{
  pkgs,
  lib,
  ...
}:
let
  tomlFormat = pkgs.formats.toml { };

  cfg = {
    # Clipboard: auto-cleanup after 3 days (4320 min)
    provider.clipboard.settings = {
      auto_cleanup = 4320;
      max_items = 1000;
      pinned_on_top = true;
    };
    # Todo: task list in ~/.cache/elephant/todo.csv
    # Type task name and press Return to create when empty.
    provider.todo.settings = {
      # Urgent time window: ±30min from scheduled time
      urgent_time_frame = 30;
      # Duck audio volume during task notifications
      duck_player_volumes = true;
      # Show creation time when no other time info available
      show_creation_time = false;
      # Time format (Go layout)
      time_format = "01-02 15:04";
      # Categories: prefix new tasks (e.g., w:fix report)
      # Change category with Ctrl+Y
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
      # Notification on task due (%TASK% = task text)
      title = "Task Due";
      body = "🔔 %TASK%";
    };
    # Audio menu (see ./audio.nix)
    provider.menus.lua."audio" = import ./audio.nix { inherit pkgs; };
    provider.menus.toml."power" = {
      name = "power";
      name_pretty = "Power";
      entries = [
        {
          text = "Shutdown";
          value = "systemctl poweroff";
        }
        {
          text = "Reboot";
          value = "systemctl reboot";
        }
        {
          text = "Suspend";
          value = "systemctl suspend";
        }
        {
          text = "Logout";
          value = "niri msg action quit";
        }
      ];
    };
    # Bluetooth menu via D-Bus (replaces built-in provider to avoid pairing
    # hangs with bluetoothctl). See ./bluetooth.nix.
    provider.menus.lua."bluetooth" = (import ./bluetooth.nix { inherit pkgs; }).menuLua;
  };

  # Config generation: services.elephant.settings writes elephant/config.toml;
  # per-provider files generated from cfg above.
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

  # Restart trigger: elephant only reads config at startup
  restartTrigger = [ (builtins.hashString "sha256" (builtins.toJSON cfg)) ];
in
{
  services.elephant = {
    enable = true;
    # Providers: curated list excluding package managers (apt/dnf/arch) and
    # protonpass. nixpkgs elephant sets ELEPHANT_PROVIDER_DIR automatically.
    # wireplumber provider requires pw-dump/wpctl (from pkgs.wireplumber).
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
      # Clean up socket file on stop
      ExecStopPost = "${pkgs.coreutils}/bin/rm -f /tmp/elephant.sock";
      X-Restart-Triggers = restartTrigger;
    };
  };

  home.packages = with pkgs; [
    fd # Required by files provider
    wireplumber # Required by wireplumber provider and audio menu (wpctl/pw-dump)
  ];
}
