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
  clipboard = [
    { action = "copy"; default = true; bind = "Return"; }
    { action = "remove"; bind = "ctrl d"; after = "AsyncClearReload"; }
    { action = "show_images_only"; label = "only images"; bind = "ctrl i"; after = "AsyncClearReload"; }
    { action = "show_text_only"; label = "only text"; bind = "ctrl i"; after = "AsyncClearReload"; }
    { action = "unpin"; bind = "ctrl p"; after = "AsyncClearReload"; }
    { action = "pin"; bind = "ctrl p"; after = "AsyncClearReload"; }
    { action = "edit"; bind = "ctrl o"; }
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
  fallback = [
    { action = "menus:open"; label = "open"; after = "Nothing"; }
    { action = "menus:default"; label = "run"; after = "Close"; }
    { action = "menus:parent"; label = "back"; bind = "Escape"; after = "Nothing"; }
    { action = "erase_history"; label = "clear hist"; bind = "ctrl h"; after = "AsyncReload"; }
  ];
  # Per-entry actions for menus. Walker registers the menus provider
  # under the name "menus" (from `elephant listproviders`), NOT
  # "menus:wifi" — so keybinds for custom per-entry actions
  # (disconnect/forget on wifi entries) must live under "menus".
  # These actions only exist on wifi entries, so the buttons only
  # appear there; other menus (power, etc.) are unaffected.
  menus = [
    { action = "menus:default"; default = true; bind = "Return"; after = "Close"; }
    { action = "disconnect"; label = "disconnect"; bind = "ctrl d"; after = "AsyncClearReload"; }
    { action = "forget"; label = "forget"; bind = "ctrl f"; after = "AsyncClearReload"; }
    { action = "rescan"; label = "rescan"; bind = "ctrl r"; after = "AsyncClearReload"; }
  ];
  dmenu = [ { action = "select"; default = true; bind = "Return"; } ];
}
