{...}: let
  # ── macOS palette ───────────────────────────────────────────────────────
  # Mimics CopyQ's native look on macOS: light gray surfaces, system blue
  # accent, rounded corners and subtle borders.
  bg = "#F5F5F7"; # main surface (macOS window gray)
  altBg = "#FFFFFF"; # alternate rows / inputs
  fg = "#1D1D1F"; # near-black text
  blue = "#0A84FF"; # macOS system blue (selection, accents)
  selFg = "#FFFFFF";
  dim = "#86868B"; # secondary label gray
  border = "#D2D2D7"; # hairline separator gray
  yellow = "#FFF3B0"; # search match highlight
  amber = "#FF9F0A"; # counters

  font = size: "SF Pro Text,Helvetica Neue,DejaVu Sans,${toString size},-1,5,50,0,0,0,0,0";

  # CopyQ's theme engine. Values are Qt style sheet fragments where each
  # declaration is prefixed with `;` (that is CopyQ's convention, see
  # `share/copyq/themes/*.css` in the package) and `''${name}` refers back to
  # another key in this same section.
  themeBody = ''
    css_template_items=items
    css_template_main_window=main_window
    css_template_menu=menu
    css_template_notification=notification

    style_main_window=true
    show_number=true
    show_scrollbars=true
    font_antialiasing=true
    use_system_icons=true

    item_spacing=1
    num_margin=4

    font="${font 10}"
    find_font="${font 10}"
    edit_font="${font 10}"
    notes_font="${font 9}"
    notification_font="${font 10}"
    num_font="${font 8}"

    bg=${bg}
    fg=${fg}
    alt_bg=${altBg}
    sel_bg=${blue}
    sel_fg=${selFg}
    find_bg=${yellow}
    find_fg=${fg}
    edit_bg=${altBg}
    edit_fg=${fg}
    notes_bg=${altBg}
    notes_fg=${fg}
    num_fg=${dim}
    num_sel_fg=${selFg}
    notification_bg=${altBg}
    notification_fg=${fg}

    menu_bar_css=";background: ''${bg};color: ''${fg};padding: 0.15em"
    menu_bar_selected_css=";background: ''${sel_bg};color: ''${sel_fg};border-radius: 4px"
    menu_bar_disabled_css=";color: ''${num_fg}"
    menu_css=";background: ''${alt_bg};color: ''${fg};border: 1px solid ${border};border-radius: 6px;padding: 4px"

    tool_bar_css=";background: ''${bg};color: ''${fg};border: 0"
    tool_button_css=";background: transparent;color: ''${fg};border: 0;border-radius: 5px;padding: 2px;font-weight: normal"
    tool_button_selected_css=";background: rgba(0,0,0,0.08);color: ''${fg};border: 0"
    tool_button_pressed_css=";background: ''${sel_bg};color: ''${sel_fg}"

    search_bar=";background: ''${edit_bg};color: ''${edit_fg};border: 1px solid ${border};border-radius: 6px;margin: 2px;padding: 0.25em"
    search_bar_focused=";border: 2px solid ${blue}"

    tab_bar_css=";background: ''${bg}"
    tab_bar_tab_selected_css=";background: ''${alt_bg};color: ''${fg};padding: 0.3em 0.9em;border: 1px solid ${border};border-bottom: 0;border-top-left-radius: 6px;border-top-right-radius: 6px"
    tab_bar_tab_unselected_css=";background: transparent;color: ${dim};padding: 0.3em 0.9em;border: 0"
    tab_bar_scroll_buttons_css=";background: ''${bg};color: ${dim};border: 0"
    tab_bar_item_counter=";color: ${amber};font-size: 6pt"
    tab_bar_sel_item_counter=";color: ${amber}"

    tab_tree_css=";background: ''${bg};color: ''${fg}"
    tab_tree_sel_item_css=";background: ''${sel_bg};color: ''${sel_fg};border-radius: 5px"
    tab_tree_item_counter=";color: ${amber};font-size: 6pt"
    tab_tree_sel_item_counter=";color: ''${sel_fg}"

    item_css=";padding: 0.35em 0.6em;border-radius: 5px"
    alt_item_css=";background: ''${alt_bg}"
    sel_item_css=";border-radius: 5px"
    cur_item_css=";border: 1px solid ${border}"
    hover_item_css=";background: rgba(0,0,0,0.04)"
    notes_css=";border: 1px solid ${border};border-radius: 5px"

    css="QScrollBar:vertical{;background: transparent;width: 10px;margin: 2px}QScrollBar::handle:vertical{;background: rgba(0,0,0,0.25);min-height: 20px;border-radius: 5px}QScrollBar::handle:vertical:hover{;background: rgba(0,0,0,0.4)}QScrollBar:horizontal{;background: transparent;height: 10px;margin: 2px}QScrollBar::handle:horizontal{;background: rgba(0,0,0,0.25);min-width: 20px;border-radius: 5px}QScrollBar::handle:horizontal:hover{;background: rgba(0,0,0,0.4)}QScrollBar::add-line,QScrollBar::sub-line,QScrollBar::add-page,QScrollBar::sub-page{;background: none;border: 0;height: 0;width: 0}"
  '';
in {
  services.copyq.enable = true;

  home.file.".config/copyq/copyq.conf".text = ''
    [Options]
    maxitems=2000
    tabs=&clipboard
    always_on_top=true
    confirm_exit=false
    close_on_unfocus=true
    activate_pastes=false
    disable_tray=false
    style=Fusion

    [Theme]
    ${themeBody}
  '';

  # Also expose the palette as a loadable theme so it shows up in
  # Preferences → Appearance → Themes as "macos".
  home.file.".config/copyq/themes/macos.ini".text = ''
    [General]
    ${themeBody}
  '';
}
