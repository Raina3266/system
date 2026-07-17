# Yazi theme — Cyberpunk palette.
# Aligned with System/home/niri/themes/{fcitx5,walker,waybar}-cyberpunk.
# Schema: https://yazi-rs.github.io/docs/configuration/theme (v26.5.6)
#
# Palette (shared across the desktop):
#   bg:     #0a0a14  dark blue-black
#   bg-mod: #141428  secondary surface
#   fg:     #cbe3e7  light cyan-white
#   pink:   #ff7edb  primary accent (selection, borders)
#   cyan:   #7afcff  secondary accent (input, highlights)
#   amber:  #f29e74  media / warnings
#   red:    #ff3333  errors / archives
#   dim:    #5c6776  muted text
let
  bg = "#0a0a14";
  bgmod = "#141428";
  fg = "#cbe3e7";
  pink = "#ff7edb";
  cyan = "#7afcff";
  amber = "#f29e74";
  red = "#ff3333";
  dim = "#5c6776";
in
{
  # ── Filetype rules ──────────────────────────────────────────────────────
  filetype.rules = [
    # Media
    { mime = "image/*"; fg = cyan; }
    { mime = "video/*"; fg = amber; }
    { mime = "audio/*"; fg = amber; }
    # Archives (red — caution color)
    { mime = "application/{,g}zip"; fg = red; }
    { mime = "application/x-{tar,bzip2,7z-compressed,xz,rar}"; fg = red; }
    # Documents
    { mime = "application/pdf"; fg = cyan; }
    { mime = "application/vnd.openxmlformats-*"; fg = pink; }
    # Code & data
    { mime = "text/*"; fg = cyan; }
    { mime = "application/json"; fg = pink; }
    { mime = "application/javascript"; fg = pink; }
    # Fallbacks
    { url = "*"; fg = dim; }
    { url = "*/"; fg = cyan; }
  ];

  # ── Manager ─────────────────────────────────────────────────────────────
  mgr = {
    cwd = { fg = cyan; bold = true; };
    find_keyword = { fg = pink; bold = true; };
    find_position = { fg = cyan; };
    marker_copied = { fg = cyan; };
    marker_cut = { fg = red; };
    marker_selected = { fg = pink; };
    count_copied = { fg = cyan; };
    count_cut = { fg = red; };
    count_selected = { fg = pink; };
    border_symbol = "│";
    border_style = { fg = pink; };
  };

  # ── Indicator bar ───────────────────────────────────────────────────────
  indicator = {
    parent = { fg = dim; };
    current = { fg = pink; };
    preview = { fg = cyan; };
  };

  # ── Tabs ────────────────────────────────────────────────────────────────
  tabs = {
    active = { fg = bg; bg = pink; bold = true; };
    inactive = { fg = dim; };
  };

  # ── Mode (status line left) ─────────────────────────────────────────────
  mode = {
    normal_main = { fg = bg; bg = pink; bold = true; };
    normal_alt = { fg = pink; bg = bgmod; };
    select_main = { fg = bg; bg = cyan; bold = true; };
    select_alt = { fg = cyan; bg = bgmod; };
    unset_main = { fg = bg; bg = dim; };
    unset_alt = { fg = dim; bg = bgmod; };
  };

  # ── Status line ─────────────────────────────────────────────────────────
  status = {
    overall = { fg = fg; bg = bg; };
    perm_type = { fg = cyan; };
    perm_read = { fg = amber; };
    perm_write = { fg = red; };
    perm_exec = { fg = pink; };
    perm_sep = { fg = dim; };
    progress_label = { bold = true; };
    progress_normal = { fg = fg; bg = bgmod; };
    progress_error = { fg = red; bg = bgmod; };
  };

  # ── Which (key hint popup) ──────────────────────────────────────────────
  which = {
    mask = { bg = bg; };
    cand = { fg = pink; bold = true; };
    rest = { fg = cyan; };
    desc = { fg = fg; };
    separator = "  ";
    separator_style = { fg = dim; };
  };

  # ── Confirm dialogs ─────────────────────────────────────────────────────
  confirm = {
    border = { fg = pink; };
    title = { fg = cyan; bold = true; };
    body = { fg = fg; };
    list = { fg = fg; };
    btn_yes = { fg = bg; bg = pink; bold = true; };
    btn_no = { fg = fg; bg = bgmod; };
    btn_labels = [ "Yes" "No" ];
  };

  # ── Spot (file info) ────────────────────────────────────────────────────
  spot = {
    border = { fg = pink; };
    title = { fg = cyan; bold = true; };
    tbl_col = { fg = pink; };
    tbl_cell = { fg = cyan; };
  };

  # ── Notify ──────────────────────────────────────────────────────────────
  notify = {
    title_info = { fg = cyan; };
    title_warn = { fg = amber; };
    title_error = { fg = red; };
  };

  # ── Pick ────────────────────────────────────────────────────────────────
  pick = {
    border = { fg = pink; };
    active = { fg = pink; bold = true; };
    inactive = { fg = dim; };
  };

  # ── Input ───────────────────────────────────────────────────────────────
  input = {
    border = { fg = pink; };
    title = { fg = cyan; };
    value = { fg = fg; };
    selected = { fg = bg; bg = pink; };
  };

  # ── Completion ──────────────────────────────────────────────────────────
  cmp = {
    border = { fg = pink; };
    active = { fg = pink; bold = true; };
    inactive = { fg = dim; };
  };

  # ── Tasks ───────────────────────────────────────────────────────────────
  tasks = {
    border = { fg = pink; };
    title = { fg = cyan; };
    hovered = { fg = pink; bold = true; };
  };

  # ── Help ────────────────────────────────────────────────────────────────
  help = {
    on = { fg = pink; bold = true; };
    run = { fg = cyan; };
    desc = { fg = fg; };
    hovered = { fg = bg; bg = pink; };
    footer = { fg = dim; };
  };
}
