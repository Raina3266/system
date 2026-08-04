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

    # Two changes to textbox-current-entry, both in source/view.c:
    #
    #  1. Add TB_WRAP so long entries wrap instead of being ellipsized.
    #
    #  2. Re-run layout when the selection changes. Auto-height is otherwise a
    #     one-way ratchet: it grows to two lines but never shrinks back to one.
    #     The height itself is computed correctly by textbox_get_desired_height,
    #     but nothing ever *asks* for it here -- textbox_text() only calls
    #     widget_update() on the TB_AUTOWIDTH path, and the window height is
    #     only recomputed in rofi_view_refilter_real(), which the
    #     selection-changed path never goes through.
    postPatch = ''
      patch -p1 <<'PATCH'
      --- a/source/view.c
      +++ b/source/view.c
      @@ -687,6 +687,13 @@
           } else {
             textbox_text(state->tb_current_entry, "");
           }
      +    widget_update(WIDGET(state->tb_current_entry)->parent);
      +    int nh = rofi_view_calculate_window_height(state);
      +    if (nh != state->height) {
      +      state->height = nh;
      +      rofi_view_calculate_window_position(state);
      +      rofi_view_window_update_size(state);
      +    }
         }
         if (state->icon_current_entry) {
           if (index < state->filtered_lines) {
      @@ -1665,7 +1665,7 @@
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
