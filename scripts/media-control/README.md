# media-control

`media-control` is a small Rust frontend around MPRIS/`playerctl` for the
repository's Rofi and Waybar setup.

```text
media-control menu
media-control waybar --watch --interval-ms 750
media-control pause-all
media-control list
```

The Rofi menu lists only `Playing` and `Paused` players. Pinned players sort
first; within each pin group, playing players sort above paused players and the
most recently started player sorts first. State is stored under
`$XDG_STATE_HOME/media-control`.

The remove action sends `Stop`, then calls MPRIS `Quit` for normal desktop
players. MPRIS does not expose a browser-tab close operation, so browsers are
only stopped by default. Set `MEDIA_CONTROL_REMOVE_HOOK` to an executable that
accepts the MPRIS player ID if compositor/browser-specific tab closing is
needed.

