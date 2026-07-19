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
  white = "#ffffff";
  fg = "#cbe3e7";
  pink = "#ff7edb";
  cyan = "#7afcff";
  red = "#ff3333";
  dim = "#5c6776";
  green = "#6af6a8";   
  yellow = "#ffe66d";  
  navy = "#5c7cfa";    
  purple = "#c792ea";  
  amber = "#f29e74";   
in
{
  # ── Filetype rules ──────────────────────────────────────────────────────
  filetype.rules = [
    # ── Directories ──
    { url = "*/"; fg = cyan; bold = true; }

    # ── Images ──
    { mime = "image/*"; fg = green; }

    # ── Video ──
    { mime = "video/*"; fg = yellow; }

    # ── Audio ──
    { mime = "audio/*"; fg = amber; }

    # ── PDF ──
    { mime = "application/pdf"; fg = purple; }

    # ── Office documents & spreadsheets ──
    { mime = "application/vnd.openxmlformats-*"; fg = navy; }
    { mime = "application/vnd.ms-*"; fg = navy; }
    { mime = "application/vnd.oasis.opendocument.*"; fg = navy; }
    { mime = "text/csv"; fg = navy; }

    # ── Archives (cyan — caution) ──
    { mime = "application/{,g}zip"; fg = cyan; }
    { mime = "application/x-{tar,bzip2,7z-compressed,xz,rar}"; fg = cyan; }
    { mime = "application/java-archive"; fg = cyan; }

    # ── Code & scripts ──
    { mime = "text/*"; fg = white; }
    { mime = "application/javascript"; fg = white; }
    { mime = "application/x-shellscript"; fg = white; }
    { mime = "application/x-python"; fg = white; }
    { mime = "application/x-rust"; fg = white; }

    # ── Data & markup formats ──
    { mime = "application/json"; fg = white; }
    { mime = "application/toml"; fg = white; }
    { mime = "application/yaml"; fg = white; }
    { mime = "application/xml"; fg = white; }
    { mime = "text/markdown"; fg = white; }
    { mime = "text/html"; fg = white; }
    { mime = "text/css"; fg = white; }

    # ── Executables & binaries ──
    { mime = "application/x-executable"; fg = dim; }
    { mime = "application/x-sharedlib"; fg = dim; }

    # ── Fallback ──
    { url = "*"; fg = fg; }
  ];

  # ── Manager ─────────────────────────────────────────────────────────────
  mgr = {
    cwd = { fg = cyan; bold = true; };
    find_keyword = { fg = red; bold = true; };
    find_position = { fg = red; };
    marker_copied = { fg = cyan; };
    marker_cut = { fg = pink; };
    marker_selected = { fg = red; };
    count_copied = { fg = cyan; };
    count_cut = { fg = pink; };
    count_selected = { fg = pink; };
    border_symbol = "│";
    border_style = { fg = pink; };
  };

  # ── Indicator bar ───────────────────────────────────────────────────────
  indicator = {
    parent = { fg = bgmod; bg = pink; bold = true; };
    current = { fg = bgmod; bg = pink; bold = true; };
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
