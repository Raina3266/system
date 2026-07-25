# Walker's `providers` option: which providers are queried by default
# and on an empty query, prefix-triggered providers, and per-provider
# action keybinds.
#
# The action keybinds are REQUIRED for the todo provider (and others)
# to function — without them, Enter/Ctrl+D etc. do nothing. Walker's
# `settings` (./default.nix) replaces its default config (imported
# from resources/config.toml) entirely, so we must re-declare the
# actions we want here.
{
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
  actions = {
    todo = [
      {
        action = "save";
        default = true;
        bind = "Return";
        after = "AsyncClearReload";
      }
      {
        action = "save_next";
        label = "save & new";
        bind = "shift Return";
        after = "AsyncClearReload";
      }
      {
        action = "delete";
        bind = "ctrl d";
        after = "AsyncClearReload";
      }
      # Used as pin/unpin: the todo provider has no pin action, but
      # "active" is a state it already sorts to the top of the list, and
      # the waybar todo module shows the active task first (top-center.nix).
      # AsyncReload so the pin marker updates without closing the menu.
      {
        action = "active";
        label = "pin";
        default = true;
        bind = "Return";
        after = "AsyncReload";
      }
      {
        action = "inactive";
        label = "unpin";
        default = true;
        bind = "Return";
        after = "AsyncReload";
      }
      {
        action = "change_category";
        bind = "ctrl y";
        label = "change category";
        after = "Nothing";
      }
      {
        action = "create";
        bind = "ctrl a";
        after = "AsyncClearReload";
      }
      {
        action = "search";
        bind = "ctrl s";
        after = "AsyncClearReload";
      }
    ];
    desktopapplications = [
      {
        action = "start";
        default = true;
        bind = "Return";
      }
      {
        action = "new_instance";
        label = "new instance";
        bind = "ctrl Return";
      }
      {
        action = "pin";
        bind = "ctrl p";
        after = "AsyncReload";
      }
      {
        action = "unpin";
        bind = "ctrl p";
        after = "AsyncReload";
      }
    ];

    clipboard = [
      {
        action = "copy";
        default = true;
        bind = "Return";
      }
      {
        action = "show_images_only";
        label = "only images";
        bind = "ctrl i";
        after = "AsyncClearReload";
      }
      {
        action = "show_text_only";
        label = "only text";
        bind = "ctrl i";
        after = "AsyncClearReload";
      }
      {
        action = "show_combined";
        label = "show all";
        bind = "ctrl i";
        after = "AsyncClearReload";
      }
      {
        action = "edit";
        bind = "ctrl o";
      }
      {
        action = "pin";
        unset = true;
      }
      {
        action = "unpin";
        unset = true;
      }
      {
        action = "remove";
        unset = true;
      }
      {
        action = "remove_all";
        unset = true;
      }
    ];
    files = [
      {
        action = "open";
        default = true;
        bind = "Return";
      }
      {
        action = "opendir";
        label = "open dir";
        bind = "ctrl Return";
      }
      {
        action = "copypath";
        label = "copy path";
        bind = "ctrl shift c";
      }
      {
        action = "copyfile";
        label = "copy file";
        bind = "ctrl c";
      }
      {
        action = "localsend";
        label = "localsend";
        bind = "ctrl l";
      }
    ];
    calc = [
      {
        action = "copy";
        default = true;
        bind = "Return";
      }
      {
        action = "delete";
        bind = "ctrl d";
        after = "AsyncReload";
      }
      {
        action = "delete_all";
        bind = "ctrl shift d";
        after = "AsyncReload";
      }
      {
        action = "save";
        bind = "ctrl s";
        after = "AsyncClearReload";
      }
    ];
    websearch = [
      {
        action = "search";
        default = true;
        bind = "Return";
      }
      {
        action = "open_url";
        label = "open url";
        default = true;
        bind = "Return";
      }
    ];
    runner = [
      {
        action = "run";
        default = true;
        bind = "Return";
      }
      {
        action = "runterminal";
        label = "run in terminal";
        bind = "shift Return";
      }
    ];
    symbols = [
      {
        action = "run_cmd";
        label = "select";
        default = true;
        bind = "Return";
      }
    ];
    unicode = [
      {
        action = "run_cmd";
        label = "select";
        default = true;
        bind = "Return";
      }
    ];
    windows = [
      {
        action = "activate";
        default = true;
        bind = "Return";
      }
    ];
    providerlist = [
      {
        action = "activate";
        default = true;
        bind = "Return";
        after = "ClearReload";
      }
    ];
    playerctl = [
      {
        action = "pause";
        label = "pause";
        bind = "Return";
        after = "Nothing";
        default = true;
      }
      {
        action = "play";
        label = "play";
        bind = "Return";
        after = "Nothing";
        default = true;
      }
      {
        action = "prev";
        label = "prev";
        bind = "ctrl p";
        after = "Nothing";
      }
      {
        action = "next";
        label = "next";
        bind = "ctrl n";
        after = "Nothing";
      }
      {
        action = "vol_up";
        label = "vol+";
        bind = "ctrl y";
        after = "Nothing";
      }
      {
        action = "vol_down";
        label = "vol-";
        bind = "ctrl h";
        after = "Nothing";
      }
      {
        action = "mute";
        label = "mute";
        bind = "ctrl m";
        after = "Nothing";
      }
      {
        action = "unmute";
        label = "unmute";
        bind = "ctrl m";
        after = "Nothing";
      }
      {
        action = "seek_back";
        label = "backward";
        bind = "ctrl b";
        after = "Nothing";
      }
      {
        action = "seek_forward";
        label = "forward";
        bind = "ctrl f";
        after = "Nothing";
      }
    ];
    wireplumber = [
      {
        action = "increase_volume";
        label = "+volume";
        bind = "ctrl y";
        after = "Nothing";
      }
      {
        action = "decrease_volume";
        label = "-volume";
        bind = "ctrl n";
        after = "Nothing";
      }
      {
        action = "mute";
        bind = "ctrl m";
        after = "Nothing";
      }
      {
        action = "unmute";
        bind = "ctrl m";
        after = "Nothing";
      }
      {
        action = "set_default_device";
        label = "set default";
        bind = "ctrl d";
        after = "Nothing";
      }
    ];
    nirisessions = [
      {
        action = "start";
        label = "start";
        default = true;
        bind = "Return";
      }
      {
        action = "start_new";
        label = "start blank";
        bind = "ctrl Return";
      }
    ];
    # Applied to any reported action a provider's own section doesn't
    # declare. `menus:default` is what elephant synthesizes for menu
    # entries that carry no Actions table of their own (e.g. the power and
    # audio menus, which drive everything off their top-level
    # `action`) -- it means "run this entry's command", hence the label.
    fallback = [
      {
        action = "menus:open";
        label = "open";
        after = "Nothing";
      }
      {
        action = "menus:default";
        label = "run";
        default = true;
        bind = "Return";
        after = "Close";
      }
      {
        action = "menus:parent";
        label = "back";
        bind = "Escape";
        after = "Nothing";
      }
      {
        action = "erase_history";
        label = "clear hist";
        bind = "ctrl h";
        after = "AsyncReload";
      }
    ];
    # Per-entry actions for menus. Elephant >= 2.x registers each menu as
    # its own provider (menus:wifi, menus:bluetooth, ...) — walker looks
    # up provider actions by that full name, so each menu needs its own
    # section. Actions not present on an entry are simply not shown, so
    # per-menu sections stay precise.
    "menus:wifi" = [
      {
        action = "menus:default";
        label = "connect";
        default = true;
        bind = "Return";
        after = "Close";
      }
      {
        action = "disconnect";
        label = "disconnect";
        bind = "ctrl d";
        after = "AsyncClearReload";
      }
      {
        action = "forget";
        label = "forget";
        bind = "ctrl f";
        after = "AsyncClearReload";
      }
    ];
    # Which buttons actually appear is decided per entry by the Actions
    # table in bluetooth.nix (e.g. power_on only on the "Bluetooth is off"
    # entry, pair only on unpaired ones); this section only supplies their
    # labels, keybinds and after-behaviour. Walker drops any action listed
    # here that the selected entry doesn't report.

    "menus:bluetooth" = [
      {
        action = "forget";
        label = "forget";
        bind = "ctrl f";
        after = "AsyncClearReload";
      }
      {
        action = "list";
        label = "list";
        bind = "ctrl l";
        after = "AsyncClearReload";
      }
      {
        action = "scan";
        label = "scan";
        bind = "ctrl s";
        after = "AsyncClearReload";
      }
      {
        action = "connect";
        label = "connect";
        default = true;
        bind = "Return";
        after = "AsyncClearReload";
      }
      {
        action = "disconnect";
        label = "disconnect";
        default = true;
        bind = "Return";
        after = "AsyncClearReload";
      }
      {
        action = "pair";
        label = "pair";
        default = true;
        bind = "Return";
        after = "AsyncClearReload";
      }
      {
        action = "power_on";
        label = "power on";
        default = true;
        bind = "Return";
        after = "AsyncClearReload";
      }
      {
        action = "power_off";
        label = "power off";
        bind = "ctrl x";
        after = "AsyncClearReload";
      }
    ];
    # Volume/mute keep the menu open and re-query so the bars update in
    # place; picking a default device closes it.
    "menus:audio" = [
      {
        action = "set_default";
        label = "select";
        default = true;
        bind = "Return";
        after = "Close";
      }
      {
        action = "vol_up";
        label = "+volume";
        bind = "ctrl y";
        after = "AsyncReload";
      }
      {
        action = "vol_down";
        label = "-volume";
        bind = "ctrl n";
        after = "AsyncReload";
      }
      {
        action = "toggle_mute";
        label = "mute";
        bind = "ctrl m";
        after = "AsyncReload";
      }
      {
        action = "toggle_kind";
        label = "in/out";
        bind = "ctrl t";
        after = "AsyncClearReload";
      }
    ];
    dmenu = [
      {
        action = "select";
        default = true;
        bind = "Return";
      }
    ];
  };
}
