# Per-provider action keybinds for walker's `providers.actions`.
#
# These are REQUIRED for the todo provider (and others) to function —
# without them, Enter/Ctrl+D etc. do nothing. The walker module's
# `config` option replaces its default (imported from
# resources/config.toml) entirely, so we must re-declare the actions
# we want here.
{
  todo = [
    { action = "save"; default = true; bind = "Return"; after = "AsyncClearReload"; }
    { action = "save_next"; label = "save & new"; bind = "shift Return"; after = "AsyncClearReload"; }
    { action = "delete"; bind = "ctrl d"; after = "AsyncClearReload"; }
    { action = "active"; default = true; bind = "Return"; after = "Nothing"; }
    { action = "inactive"; default = true; bind = "Return"; after = "Nothing"; }
    { action = "done"; bind = "ctrl f"; after = "Nothing"; }
    { action = "change_category"; bind = "ctrl y"; label = "change category"; after = "Nothing"; }
    { action = "clear"; bind = "ctrl x"; after = "AsyncClearReload"; }
    { action = "create"; bind = "ctrl a"; after = "AsyncClearReload"; }
    { action = "search"; bind = "ctrl s"; after = "AsyncClearReload"; }
  ];
  desktopapplications = [
    { action = "start"; default = true; bind = "Return"; }
    { action = "new_instance"; label = "new instance"; bind = "ctrl Return"; }
    { action = "pin"; bind = "ctrl p"; after = "AsyncReload"; }
    { action = "unpin"; bind = "ctrl p"; after = "AsyncReload"; }
  ];
  # pin / unpin / remove / remove_all are DISABLED via `unset` because
  # they deadlock elephant (2.22.0 and master as of 2026-07). Their
  # handlers in internal/providers/clipboard/clipboard.go take the
  # provider mutex and then call saveToFile(), which locks the same
  # non-reentrant sync.Mutex again:
  #
  #   case ActionPin:
  #     mu.Lock()
  #     ...
  #       saveToFile()   // -> mu.Lock() again, blocks forever
  #
  # The activation goroutine wedges holding mu, so every later Query()
  # blocks too and the clipboard appears permanently empty until
  # elephant is restarted. (cleanup() in setup.go gets this right by
  # calling saveToFileLocked() instead — that one-word change is the
  # upstream fix, not yet reported.)
  #
  # `unset` is required rather than just omitting them: walker merges
  # this section over its built-in defaults per action name, so deleted
  # entries would otherwise be reinstated from resources/config.toml.
  # Walker only renders/binds an action present in BOTH the config and
  # the item, so unsetting here makes the deadlock unreachable.
  #
  # To restore them, drop the four `unset` entries below (and ideally
  # patch elephant first).
  clipboard = [
    { action = "copy"; default = true; bind = "Return"; }
    { action = "show_images_only"; label = "only images"; bind = "ctrl i"; after = "AsyncClearReload"; }
    { action = "show_text_only"; label = "only text"; bind = "ctrl i"; after = "AsyncClearReload"; }
    { action = "show_combined"; label = "show all"; bind = "ctrl i"; after = "AsyncClearReload"; }
    { action = "edit"; bind = "ctrl o"; }
    { action = "pin"; unset = true; }
    { action = "unpin"; unset = true; }
    { action = "remove"; unset = true; }
    { action = "remove_all"; unset = true; }
  ];
  files = [
    { action = "open"; default = true; bind = "Return"; }
    { action = "opendir"; label = "open dir"; bind = "ctrl Return"; }
    { action = "copypath"; label = "copy path"; bind = "ctrl shift c"; }
    { action = "copyfile"; label = "copy file"; bind = "ctrl c"; }
    { action = "localsend"; label = "localsend"; bind = "ctrl l"; }
  ];
  calc = [
    { action = "copy"; default = true; bind = "Return"; }
    { action = "delete"; bind = "ctrl d"; after = "AsyncReload"; }
    { action = "delete_all"; bind = "ctrl shift d"; after = "AsyncReload"; }
    { action = "save"; bind = "ctrl s"; after = "AsyncClearReload"; }
  ];
  websearch = [
    { action = "search"; default = true; bind = "Return"; }
    { action = "open_url"; label = "open url"; default = true; bind = "Return"; }
  ];
  runner = [
    { action = "run"; default = true; bind = "Return"; }
    { action = "runterminal"; label = "run in terminal"; bind = "shift Return"; }
  ];
  symbols = [
    { action = "run_cmd"; label = "select"; default = true; bind = "Return"; }
  ];
  unicode = [
    { action = "run_cmd"; label = "select"; default = true; bind = "Return"; }
  ];
  windows = [
    { action = "activate"; default = true; bind = "Return"; }
  ];
  providerlist = [
    { action = "activate"; default = true; bind = "Return"; after = "ClearReload"; }
  ];
  playerctl = [
    { action = "pause"; label = "pause"; bind = "Return"; after = "Nothing"; default = true; }
    { action = "play"; label = "play"; bind = "Return"; after = "Nothing"; default = true; }
    { action = "prev"; label = "prev"; bind = "ctrl p"; after = "Nothing"; }
    { action = "next"; label = "next"; bind = "ctrl n"; after = "Nothing"; }
    { action = "vol_up"; label = "vol+"; bind = "ctrl y"; after = "Nothing"; }
    { action = "vol_down"; label = "vol-"; bind = "ctrl h"; after = "Nothing"; }
    { action = "mute"; label = "mute"; bind = "ctrl m"; after = "Nothing"; }
    { action = "unmute"; label = "unmute"; bind = "ctrl m"; after = "Nothing"; }
    { action = "seek_back"; label = "backward"; bind = "ctrl b"; after = "Nothing"; }
    { action = "seek_forward"; label = "forward"; bind = "ctrl f"; after = "Nothing"; }
  ];
  wireplumber = [
    { action = "increase_volume"; label = "+volume"; bind = "ctrl y"; after = "Nothing"; }
    { action = "decrease_volume"; label = "-volume"; bind = "ctrl n"; after = "Nothing"; }
    { action = "mute"; bind = "ctrl m"; after = "Nothing"; }
    { action = "unmute"; bind = "ctrl m"; after = "Nothing"; }
    { action = "set_default_device"; label = "set default"; bind = "ctrl d"; after = "Nothing"; }
  ];
  nirisessions = [
    { action = "start"; label = "start"; default = true; bind = "Return"; }
    { action = "start_new"; label = "start blank"; bind = "ctrl Return"; }
  ];
  # Applied to any reported action a provider's own section doesn't
  # declare. `menus:default` is what elephant synthesizes for menu
  # entries that carry no Actions table of their own (e.g. the power and
  # audio-sink menus, which drive everything off their top-level
  # `action`) -- it means "run this entry's command", hence the label.
  fallback = [
    { action = "menus:open"; label = "open"; after = "Nothing"; }
    { action = "menus:default"; label = "run"; default = true; bind = "Return"; after = "Close"; }
    { action = "menus:parent"; label = "back"; bind = "Escape"; after = "Nothing"; }
    { action = "erase_history"; label = "clear hist"; bind = "ctrl h"; after = "AsyncReload"; }
  ];
  # Per-entry actions for menus. Elephant >= 2.x registers each menu as
  # its own provider (menus:wifi, menus:bluetooth, ...) — walker looks
  # up provider actions by that full name, so each menu needs its own
  # section. Actions not present on an entry are simply not shown, so
  # per-menu sections stay precise.
  "menus:wifi" = [
    { action = "menus:default"; label = "connect"; default = true; bind = "Return"; after = "Close"; }
    { action = "disconnect"; label = "disconnect"; bind = "ctrl d"; after = "AsyncClearReload"; }
    { action = "forget"; label = "forget"; bind = "ctrl f"; after = "AsyncClearReload"; }
  ];
  # Which buttons actually appear is decided per entry by the Actions
  # table in bluetooth.nix (e.g. power_on only on the "Bluetooth is off"
  # entry, pair only on unpaired ones); this section only supplies their
  # labels, keybinds and after-behaviour. Walker drops any action listed
  # here that the selected entry doesn't report.

  "menus:bluetooth" = [
    { action = "forget"; label = "forget"; bind = "ctrl f"; after = "AsyncClearReload"; }
    # scan and list coexist on an entry while scanning, so they need
    # distinct keys (unlike the mutually-exclusive pin/unpin pattern,
    # where walker picks whichever the entry actually reports).
    { action = "list"; label = "list"; bind = "ctrl l"; after = "AsyncClearReload"; }
    { action = "scan"; label = "scan"; bind = "ctrl s"; after = "AsyncClearReload"; }
    { action = "connect"; label = "connect"; default = true; bind = "Return"; after = "AsyncClearReload"; }
    { action = "disconnect"; label = "disconnect"; default = true; bind = "Return"; after = "AsyncClearReload"; }
    { action = "pair"; label = "pair"; default = true; bind = "Return"; after = "AsyncClearReload"; }
    { action = "power_on"; label = "power on"; default = true; bind = "Return"; after = "AsyncClearReload"; }
    { action = "power_off"; label = "power off"; bind = "ctrl x"; after = "AsyncClearReload"; }
  ];
  # Volume/mute keep the menu open and re-query so the bars update in
  # place; picking a default device closes it.
  "menus:audio-sink" = [
    { action = "set_default"; label = "select"; default = true; bind = "Return"; after = "Close"; }
    { action = "vol_up"; label = "+volume"; bind = "ctrl y"; after = "AsyncReload"; }
    { action = "vol_down"; label = "-volume"; bind = "ctrl n"; after = "AsyncReload"; }
    { action = "toggle_mute"; label = "mute"; bind = "ctrl m"; after = "AsyncReload"; }
    { action = "toggle_kind"; label = "in/out"; bind = "ctrl t"; after = "AsyncClearReload"; }
  ];
  "menus:power" = [
    { action = "menus:default"; label = "run"; default = true; bind = "Return"; after = "Close"; }
  ];
  dmenu = [ { action = "select"; default = true; bind = "Return"; } ];
}
