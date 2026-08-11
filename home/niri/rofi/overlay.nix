final: prev:

let
  # Rofi commit containing Wayland click-to-exit support from PR #2272.
  rofiRevision = "6d2a5281e45dee92dfbdaf6f9ba6081c4c608682";
  rofiSourceHash = "sha256-4F76JPNaM43DgnM+F0WoYvL5aBbyPSZt3q0YWKAQ9Zs=";

  # Extend -on-selection-changed with two placeholders used by the clipboard
  # editor. {completion} is the stable clipboard item ID and
  # {selection-serial} lets Rust reject stale callbacks during fast navigation.
  selectionChangedCompletionPatch = final.writeText
    "on-selection-changed-completion.patch"
    ''
      diff --git a/doc/rofi-actions.5.markdown b/doc/rofi-actions.5.markdown
      --- a/doc/rofi-actions.5.markdown
      +++ b/doc/rofi-actions.5.markdown
      @@ -16,7 +16,10 @@ Following is the list of rofi flags for specifying custom commands or scripts t
      ${" "}
       `-on-selection-changed` *cmd*
      ${" "}
      -Command or script to run when the current selection changes. Selected text is forwarded to the command replacing the pattern *{entry}*.
      +Command or script to run when the current selection changes. Selected display
      +text replaces *{entry}*. For script modes, the original row value replaces
      +*{completion}*. A monotonically increasing callback number replaces
      +*{selection-serial}*.
      ${" "}
       `-on-entry-accepted` *cmd*
      ${" "}
      diff --git a/source/modes/script.c b/source/modes/script.c
      --- a/source/modes/script.c
      +++ b/source/modes/script.c
      @@ -508,6 +508,11 @@ static char *_get_display_value(const Mode *sw, unsigned int selected_line,
         }
       }
      ${" "}
      +static char *script_get_completion(const Mode *sw, unsigned int selected_line) {
      +  ScriptModePrivateData *pd = sw->private_data;
      +  return g_strdup(pd->cmd_list[selected_line].entry);
      +}
      +
       static int script_token_match(const Mode *sw, rofi_int_matcher **tokens,
                                     unsigned int index) {
         ScriptModePrivateData *rmpd = sw->private_data;
      @@ -678,7 +683,8 @@ Mode *script_mode_parse_setup(const char *str) {
           sw->_token_match = script_token_match;
           sw->_get_message = script_get_message;
           sw->_get_icon = script_get_icon;
      -    sw->_get_completion = NULL, sw->_preprocess_input = NULL,
      +    sw->_get_completion = script_get_completion;
      +    sw->_preprocess_input = NULL;
           sw->_get_display_value = _get_display_value;
           sw->type = MODE_TYPE_SWITCHER;
           return sw;
      @@ -702,7 +708,8 @@ Mode *script_mode_parse_setup(const char *str) {
           sw->_token_match = script_token_match;
           sw->_get_message = script_get_message;
           sw->_get_icon = script_get_icon;
      -    sw->_get_completion = NULL, sw->_preprocess_input = NULL,
      +    sw->_get_completion = script_get_completion;
      +    sw->_preprocess_input = NULL;
           sw->_get_display_value = _get_display_value;
           sw->type = MODE_TYPE_SWITCHER;
      ${" "}
      diff --git a/source/view.c b/source/view.c
      --- a/source/view.c
      +++ b/source/view.c
      @@ -652,5 +652,7 @@ inline static void rofi_view_nav_last(RofiViewState *state) {
         listview_set_selected(state->list_view, -1);
       }
      +static guint64 selection_changed_serial = 0;
      +
       static void selection_changed_user_callback(unsigned int index,
                                                   RofiViewState *state) {
         if (config.on_selection_changed == NULL)
      @@ -660,12 +662,18 @@ static void selection_changed_user_callback(unsigned int index,
         int fstate = 0;
         char *text = mode_get_display_value(state->sw, state->line_map[index],
                                             &fstate, NULL, TRUE);
      +  char *completion = mode_get_completion(state->sw, state->line_map[index]);
      +  char *serial = g_strdup_printf("%" G_GUINT64_FORMAT,
      +                                 ++selection_changed_serial);
         char **args = NULL;
         int argv = 0;
      -  helper_parse_setup(config.on_selection_changed, &args, &argv, "{entry}", text,
      -                     (char *)0);
      +  helper_parse_setup(config.on_selection_changed, &args, &argv,
      +                     "{entry}", text, "{completion}", completion,
      +                     "{selection-serial}", serial, (char *)0);
         if (args != NULL)
           helper_execute(NULL, args, "", config.on_selection_changed, NULL);
      +  g_free(serial);
      +  g_free(completion);
         g_free(text);
       }
       static void selection_changed_callback(G_GNUC_UNUSED listview *lv,
    '';

  # Rofi normally preserves a visible list index during refiltering. Opt in to
  # preserving the underlying entry instead so editing a clipboard search does
  # not move the highlight to another history item.
  preserveFilteredSelectionPatch = final.writeText
    "preserve-filtered-selection.patch"
    ''
      diff --git a/source/view.c b/source/view.c
      --- a/source/view.c
      +++ b/source/view.c
      @@ -777,6 +777,16 @@ static gboolean rofi_view_refilter_real(RofiViewState *state) {
         if (state->sw == NULL) {
           return G_SOURCE_REMOVE;
         }
      +  const gboolean preserve_filter_selection =
      +      !state->reload &&
      +      g_strcmp0(g_getenv("ROFI_PRESERVE_SELECTION_ON_FILTER"), "true") == 0;
      +  unsigned int selected_line_before_filter = UINT32_MAX;
      +  if (preserve_filter_selection && state->line_map != NULL) {
      +    unsigned int selected = listview_get_selected(state->list_view);
      +    if (selected < state->filtered_lines) {
      +      selected_line_before_filter = state->line_map[selected];
      +    }
      +  }
         GTimer *timer = g_timer_new();
         TICK_N("Filter start");
         if (state->reload) {
      @@ -876,7 +886,15 @@ static gboolean rofi_view_refilter_real(RofiViewState *state) {
         }
         TICK_N("Filter matching done");
         listview_set_num_elements(state->list_view, state->filtered_lines);
      +  if (selected_line_before_filter != UINT32_MAX) {
      +    for (unsigned int i = 0; i < state->filtered_lines; i++) {
      +      if (state->line_map[i] == selected_line_before_filter) {
      +        listview_set_selected(state->list_view, i);
      +        break;
      +      }
      +    }
      +  }
      ${" "}
         if (state->tb_filtered_rows) {
           char *r = g_strdup_printf("%u", state->filtered_lines);
           textbox_text(state->tb_filtered_rows, r);
    '';

  # Rofi normally takes exclusive layer-shell keyboard focus, which prevents
  # the clipboard's companion editor from receiving keys after it is clicked.
  # Keep the upstream default unless this opt-in environment variable is set.
  onDemandKeyboardPatch = final.writeText
    "wayland-on-demand-keyboard.patch"
    ''
      diff --git a/source/wayland/display.c b/source/wayland/display.c
      --- a/source/wayland/display.c
      +++ b/source/wayland/display.c
      @@ -1768,6 +1768,12 @@ static gboolean wayland_display_late_setup(void) {
                                              ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
                                              ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
         zwlr_layer_surface_v1_set_size(wayland->wlr_surface, 0, 0);
      -  zwlr_layer_surface_v1_set_keyboard_interactivity(wayland->wlr_surface, 1);
      +  uint32_t keyboard_interactivity = 1;
      +  if (g_strcmp0(g_getenv("ROFI_WAYLAND_KEYBOARD_MODE"), "on-demand") == 0) {
      +    // Version 4 of wlr-layer-shell assigns 2 to the on-demand mode.
      +    keyboard_interactivity = 2;
      +  }
      +  zwlr_layer_surface_v1_set_keyboard_interactivity(
      +      wayland->wlr_surface, keyboard_interactivity);
         zwlr_layer_surface_v1_add_listener(
             wayland->wlr_surface, &wayland_layer_shell_surface_listener, NULL);
     '';
in
{
  rofi-unwrapped = prev.rofi-unwrapped.overrideAttrs (oldAttrs: {
    version = "2.0.0-dev";

    src = final.fetchFromGitHub {
      owner = "davatorium";
      repo = "rofi";
      rev = rofiRevision;
      fetchSubmodules = true;
      hash = rofiSourceHash;
    };

    patches = (oldAttrs.patches or [ ]) ++ [
      selectionChangedCompletionPatch
      preserveFilteredSelectionPatch
      onDemandKeyboardPatch
    ];
  });
}
