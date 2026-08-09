final: prev: {
  # Pinned to the merge commit of PR #2272, which implements
  # click-to-exit on Wayland (fullscreen capture surface).
  rofi-unwrapped = prev.rofi-unwrapped.overrideAttrs (old: {
    version = "2.0.0-dev";
    src = final.fetchFromGitHub {
      owner = "davatorium";
      repo = "rofi";
      rev = "6d2a5281e45dee92dfbdaf6f9ba6081c4c608682";
      fetchSubmodules = true;
      hash = "sha256-4F76JPNaM43DgnM+F0WoYvL5aBbyPSZt3q0YWKAQ9Zs=";
    };

    # Initialize the script-mode icon fallback state so the first row can load
    # its preview. Allow long current-entry labels to wrap, and relayout their
    # parent when the selection changes so wrapped lines are not clipped.
    postPatch = ''
      patch -p1 <<'PATCH'
      --- a/source/modes/script.c
      +++ b/source/modes/script.c
      @@ -291,6 +291,7 @@
                   retv[(*length)].icon_fetch_uid = 0;
                   retv[(*length)].icon_fetch_size = 0;
                   retv[(*length)].icon_fetch_scale = 0;
      +            retv[(*length)].icon_fallback_index = 0;
                   retv[(*length)].nonselectable = FALSE;
                   retv[(*length)].permanent = FALSE;
                   if (buf_length > 0 && (read_length > (ssize_t)buf_length)) {
      --- a/source/view.c
      +++ b/source/view.c
      @@ -687,6 +687,7 @@
           } else {
             textbox_text(state->tb_current_entry, "");
           }
      +    widget_update(WIDGET(state->tb_current_entry)->parent);
         }
         if (state->icon_current_entry) {
           if (index < state->filtered_lines) {
      @@ -1665,7 +1666,7 @@
         } else if (strcmp(name, "textbox-current-entry") == 0) {
           state->tb_current_entry =
               textbox_create(parent_widget, WIDGET_TYPE_TEXTBOX_TEXT, name,
      -                       TB_MARKUP | TB_AUTOHEIGHT, NORMAL, "", 0, 0);
      +                       TB_MARKUP | TB_AUTOHEIGHT | TB_WRAP, NORMAL, "", 0, 0);
           box_add((box *)parent_widget, WIDGET(state->tb_current_entry), FALSE);
           defaults = NULL;
         } else if (strcmp(name, "icon-current-entry") == 0) {
      PATCH
    '';
  });
}
