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
      duckdb # data files (used by duckdb plugin)
      chafa # image preview fallback in terminal
      catdoc # .doc preview
      xlsx2csv # spreadsheet preview
      fontpreview # font preview

      # Plugin runtime dependencies
      trash-cli # recycle-bin plugin
      zip # compress plugin (.zip)
      p7zip # compress plugin (.7z, password-protected zip)
      util-linux # mount plugin (lsblk, eject)
      udisks # mount plugin (udisksctl)
    ];

    # ──────────────────────────────────────────────────────────────────────
    # Plugins — each entry is linked to $XDG_CONFIG_HOME/yazi/plugins/<name>.yazi
    # ──────────────────────────────────────────────────────────────────────
    plugins = {
      # 1. Advanced & Specialized Previews
      "mime-ext" = yp.mime-ext; # fast mime detection by extension
      "rich-preview" = yp.rich-preview; # rich previews for various types
      "glow" = yp.glow; # markdown rendering
      "mediainfo" = yp.mediainfo; # media metadata preview
      "duckdb" = yp.duckdb; # SQL/Parquet/CSV via duckdb
      "lsar" = yp.lsar; # archive contents listing
      "office" = yp.office; # .docx/.xlsx/.pptx preview
      "piper" = yp.piper; # pipe any shell command as previewer
      "allmytoes" = yp.allmytoes; # freedesktop thumbnail generation
      "convert" = yp.convert; # image format conversion

      # 2. Smart File Manipulation & Integration
      "smart-filter" = yp.smart-filter;
      "smart-enter" = yp.smart-enter;
      "smart-paste" = yp.smart-paste;
      "bookmarks" = yp.bookmarks;
      "full-border" = yp.full-border;
      "ouch" = yp.ouch; # archive compress/extract
      "rsync" = yp.rsync;
      "sudo" = yp.sudo;
      "diff" = yp.diff;

      # 3. UI Customization & Quality of Life
      "yatline" = yp.yatline; # custom header/status lines
      "git" = yp.git; # git status in listings
      "githead" = yp.githead; # git branch in header
      "toggle-pane" = yp.toggle-pane;
      "zoom" = yp.zoom; # zoom preview pane

      "jump-to-char" = yp.jump-to-char;
      "easyjump" = yp.easyjump;

      # File/system operations & integrations
      "mount" = yp.mount; # disk mount/unmount/eject manager
      "yafg" = yp.yafg; # ripgrep+fzf content search
      "recycle-bin" = yp.recycle-bin; # trash management
      "compress" = yp.compress; # archive creation
      "chmod" = yp.chmod; # chmod selected files

      # UI Customization
      "starship" = yp.starship; # starship prompt in header
    };

    # ──────────────────────────────────────────────────────────────────────
    # main.lua — register every plugin so it can be referenced from keymap.
    # ──────────────────────────────────────────────────────────────────────
    initLua = ./main.lua;

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
          {
            url = "*";
            run = "git";
            group = "git";
          }
          {
            url = "*/";
            run = "git";
            group = "git";
          }
        ];
        # 1. Specialized previewers.
        prepend_previewers = [
          {
            mime = "text/markdown";
            run = "glow";
          }
          {
            mime = "text/csv";
            run = "duckdb";
          }
          {
            mime = "application/json";
            run = "duckdb";
          }
          # Office documents
          {
            mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
            run = "office";
          }
          {
            mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
            run = "office";
          }
          {
            mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
            run = "office";
          }
          # Archives (lsar lists contents)
          {
            mime = "application/{,g}zip";
            run = "lsar";
          }
          {
            mime = "application/x-{tar,bzip2,7z-compressed,xz,rar}";
            run = "lsar";
          }
          # Data files via duckdb
          {
            mime = "application/vnd.ms-excel";
            run = "duckdb";
          }
          {
            url = "*.parquet";
            run = "duckdb";
          }
        ];
      };
      
      # ──────────────────────────────────────────────────────────────────────
      # File opener configuration
      # ──────────────────────────────────────────────────────────────────────
      opener.text_editor = [
        { run = "zed \"$@\""; desc = "Edit in Zed"; for = "linux"; }
      ];
      opener.media_player = [
        { run = "vlc \"$@\""; desc = "Play in VLC"; for = "linux"; }
      ];
      opener.pdf_viewer = [
        { run = "google-chrome \"$@\""; desc = "Open in Chrome"; for = "linux"; }
      ];
      opener.office_suite = [
        { run = "onlyoffice-desktopeditors \"$@\""; desc = "Edit in OnlyOffice"; for = "linux"; }
      ];
      
      # File type associations
      open.prepend_rules = [
        # Text files - Zed
        { mime = "text/*"; use = "text_editor"; }
        { url = "*.{txt,md,json,yaml,yml,toml,conf,config,log,sh,py,rs,js,ts,html,css,xml,svg,nix,lua}"; use = "text_editor"; }
        
        # Media files - VLC
        { mime = "video/*"; use = "media_player"; }
        { mime = "audio/*"; use = "media_player"; }
        { url = "*.{mp4,mkv,avi,mov,mp3,flac,wav,ogg,m4a,webm,3gp,wmv,flv,m4v,ogv}"; use = "media_player"; }
        
        # PDF files - Chrome
        { mime = "application/pdf"; use = "pdf_viewer"; }
        { url = "*.pdf"; use = "pdf_viewer"; }
        
        # Office documents - OnlyOffice
        { mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"; use = "office_suite"; }  # .docx
        { mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"; use = "office_suite"; }  # .xlsx
        { mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation"; use = "office_suite"; }  # .pptx
        { mime = "application/msword"; use = "office_suite"; }  # .doc
        { mime = "application/vnd.ms-powerpoint"; use = "office_suite"; }  # .ppt
        { mime = "application/vnd.ms-excel"; use = "office_suite"; }  # .xls
        { url = "*.{doc,docx,ppt,pptx,csv,xls,xlsx,odt,ods,odp}"; use = "office_suite"; }
      ];
    };

    # ──────────────────────────────────────────────────────────────────────
    # theme.toml — UI customization (filetype colors, etc.)
    # ──────────────────────────────────────────────────────────────────────
    theme = import ./theme.nix;



    # ──────────────────────────────────────────────────────────────────────
    # keymap.toml — plugin key bindings.
    # Chords are used to avoid shadowing single-key defaults.
    # ──────────────────────────────────────────────────────────────────────
    keymap = {
      mgr.prepend_keymap = [
        # mount — disk mount manager
        {
          on = "M";
          run = "plugin mount";
          desc = "Mount manager";
        }
        # yafg — fuzzy grep file contents
        {
          on = [
            "F"
            "G"
          ];
          run = "plugin yafg";
          desc = "Grep file contents (rg+fzf)";
        }
        # recycle-bin — trash menu
        {
          on = [
            "R"
            "b"
          ];
          run = "plugin recycle-bin";
          desc = "Open Recycle Bin menu";
        }
        # compress — archive selected files
        {
          on = [
            "c"
            "a"
            "a"
          ];
          run = "plugin compress";
          desc = "Archive selected files";
        }
        {
          on = [
            "c"
            "a"
            "p"
          ];
          run = "plugin compress -p";
          desc = "Archive (password)";
        }
        {
          on = [
            "c"
            "a"
            "h"
          ];
          run = "plugin compress -ph";
          desc = "Archive (password+header)";
        }
        {
          on = [
            "c"
            "a"
            "l"
          ];
          run = "plugin compress -l";
          desc = "Archive (compression level)";
        }
        # chmod — change file mode
        {
          on = [
            "c"
            "m"
          ];
          run = "plugin chmod";
          desc = "Chmod on selected files";
        }
      ];
    };
  };
}
