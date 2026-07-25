# Walker providers config: default/empty query providers, prefixes, and keybinds.
# Action keybinds are required (walker's settings replaces default config entirely).
# Without these, providers like todo won't respond to Enter/Ctrl+D.
{
  # Default providers (shown on launch and empty query)
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
      # Pin/unpin via "active"/"inactive" (sorted to top, shown first in waybar)
      # AsyncReload keeps menu open while updating
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
    # Fallback for actions not in provider-specific sections.
    # menus:default = run entry's command (for menus without Actions table)
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
    # Menu-specific actions: each menu is a provider (menus:bluetooth, etc.)
    # Only actions present on an entry are shown.
    # Actions defined here; visibility controlled by bluetooth.nix Actions table
    # per entry (e.g., power_on only when off, pair only on unpaired).

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
    # Volume/mute: keep open and re-query | Select default: close menu
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
