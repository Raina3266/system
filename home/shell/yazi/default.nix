# Yazi configuration.
#
# Plugin system is organized into three categories:
#   1. Advanced & Specialized Previews  — previewers for code, archives, etc.
#   2. Smart File Manipulation & Integration — bulk ops, smart open, bookmarks.
#   3. UI Customization & Quality of Life — status bars, flavor, keymap tweaks.
#
# Plugins come from nixpkgs' yaziPlugins bundle (pinned to match the yazi
# package version) and are linked into $XDG_CONFIG_HOME/yazi/plugins/<name>.yazi.
# Each plugin must also be add()'ed in init.lua before it can be referenced.
{
  pkgs,
  lib,
  ...
}:
let
  # The yaziPlugins bundle ships plugins matching the yazi package version.
  # Using it (rather than fetching plugins ad-hoc) avoids version drift.
  yp = pkgs.yaziPlugins;
in
{
  programs.yazi = {
    enable = true;
    enableFishIntegration = true;
    enableBashIntegration = true;

    # Tools yazi can shell out to for previews / file ops.
    extraPackages = with pkgs; [
      ffmpeg
      poppler-utils # PDF preview (pdftoppm)
      imagemagick
      unar # archive preview/extract
      jq # JSON preview
      fd
      ripgrep
      zoxide
      fzf
      exiftool
      mediainfo
      glow # markdown rendering (used by glow plugin)
      miller # CSV/TSV/JSON (used by miller plugin)
      duckdb # data files (used by duckdb plugin)
      chafa # image preview fallback in terminal
      catdoc # .doc preview
      xlsx2csv # spreadsheet preview
      fontpreview # font preview
    ];

    # ──────────────────────────────────────────────────────────────────────
    # Plugins — each entry is linked to $XDG_CONFIG_HOME/yazi/plugins/<name>.yazi
    # ──────────────────────────────────────────────────────────────────────
    plugins = {
      # 1. Advanced & Specialized Previews
      "mime-ext" = yp.mime-ext;       # fast mime detection by extension
      "rich-preview" = yp.rich-preview; # rich previews for various types
      "glow" = yp.glow;               # markdown rendering
      "miller" = yp.miller;           # CSV/TSV/JSON tabular preview
      "mediainfo" = yp.mediainfo;     # media metadata preview
      "duckdb" = yp.duckdb;           # SQL/Parquet/CSV via duckdb
      "lsar" = yp.lsar;               # archive contents listing
      "office" = yp.office;           # .docx/.xlsx/.pptx preview
      "piper" = yp.piper;             # pipe any shell command as previewer
      "allmytoes" = yp.allmytoes;     # freedesktop thumbnail generation
      "convert" = yp.convert;         # image format conversion

      # 2. Smart File Manipulation & Integration
      "smart-filter" = yp.smart-filter;
      "smart-enter" = yp.smart-enter;
      "smart-paste" = yp.smart-paste;
      "bookmarks" = yp.bookmarks;
      "full-border" = yp.full-border;
      "ouch" = yp.ouch;               # archive compress/extract
      "rsync" = yp.rsync;
      "sudo" = yp.sudo;
      "diff" = yp.diff;

      # 3. UI Customization & Quality of Life
      "yatline" = yp.yatline;         # custom header/status lines
      "git" = yp.git;                 # git status in listings
      "githead" = yp.githead;         # git branch in header
      "toggle-pane" = yp.toggle-pane;
      "zoom" = yp.zoom;               # zoom preview pane
      "relative-motions" = yp.relative-motions;
      "jump-to-char" = yp.jump-to-char;
      "easyjump" = yp.easyjump;
    };

    # ──────────────────────────────────────────────────────────────────────
    # init.lua — register every plugin so it can be referenced from keymap.
    # ──────────────────────────────────────────────────────────────────────
    initLua = ./init.lua;

    # ──────────────────────────────────────────────────────────────────────
    # yazi.toml — built-in config + plugin feature flags.
    # ──────────────────────────────────────────────────────────────────────
    settings = {
      mgr = {
        show_hidden = true;
        sort_by = "alphabetical";
        sort_dir_first = true;
        sort_reverse = false;
        linemode = "size";
        show_symlink = true;
      };

      # Plugin feature flags (read by plugins via ya.db).
      plugin = {
        # 1. Use the faster extension-based mime fetcher.
        #    `mime-ext` was split into `mime-ext.local` and `mime-ext.remote`.
        #    `group = "mime"` makes these replace the built-in mime fetcher
        #    (only the first matching fetcher in a group runs).
        prepend_fetchers = [
          {
            "if" = "!mime";
            url = "local://*";
            run = "mime-ext.local";
            prio = "high";
            group = "mime";
          }
          {
            "if" = "!mime";
            url = "remote://*";
            run = "mime-ext.remote";
            prio = "high";
            group = "mime";
          }
          # 3. Git status fetcher (feeds the `git` linemode + githead).
          { url = "*"; run = "git"; group = "git"; }
          { url = "*/"; run = "git"; group = "git"; }
        ];
        # 1. Specialized previewers.
        prepend_previewers = [
          { mime = "text/markdown"; run = "glow"; }
          { mime = "text/csv"; run = "miller"; }
          { mime = "application/json"; run = "miller"; }
          # Office documents
          { mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"; run = "office"; }
          { mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"; run = "office"; }
          { mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation"; run = "office"; }
          # Archives (lsar lists contents)
          { mime = "application/{,g}zip"; run = "lsar"; }
          { mime = "application/x-{tar,bzip2,7z-compressed,xz,rar}"; run = "lsar"; }
          # Data files via duckdb
          { mime = "application/vnd.ms-excel"; run = "duckdb"; }
          { url = "*.parquet"; run = "duckdb"; }
        ];
      };
    };

    # ──────────────────────────────────────────────────────────────────────
    # theme.toml — UI customization (filetype colors, etc.)
    # ──────────────────────────────────────────────────────────────────────
    theme = import ./theme.nix;
  };
}
