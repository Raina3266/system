# Yazi core settings configuration.
# This includes manager settings and plugin feature flags.
{
  mgr = {
    show_hidden = true;
    sort_by = "alphabetical";
    sort_dir_first = true;
    sort_reverse = false;
    linemode = "size";
    show_symlink = true;
    # Add more vertical space for file list (middle pane gets more height)
    ratio = [ 3 3 3 ];
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
  # Note: for each opener, the first entry is the default (used by plain
  # "open"); the rest only show up in the "Open with..." (<Enter>) menu.
  opener.text_editor = [
    {
      run = "zed \"$@\"";
      desc = "Edit in Zed";
      for = "linux";
    }
    {
      run = "nvim \"$@\"";
      desc = "Edit in Neovim";
      for = "linux";
      block = true;
    }
    {
      run = "xdg-open \"$@\"";
      desc = "Open with default app";
      for = "linux";
    }
  ];
  opener.media_player = [
    {
      run = "vlc \"$@\"";
      desc = "Play in VLC";
      for = "linux";
    }
    {
      run = "kid3 \"$@\"";
      desc = "Edit tags in Kid3";
      for = "linux";
    }
    {
      run = "xdg-open \"$@\"";
      desc = "Open with default app";
      for = "linux";
    }
  ];
  opener.pdf_viewer = [
    {
      run = "google-chrome --new-window \"$@\"";
      desc = "Open in Chrome (New Window)";
      for = "linux";
    }
    {
      run = "stirling-pdf \"$@\"";
      desc = "Open in Stirling PDF";
      for = "linux";
    }
    {
      run = "xdg-open \"$@\"";
      desc = "Open with default app";
      for = "linux";
    }
  ];
  opener.office_suite = [
    {
      run = "onlyoffice-desktopeditors \"$@\"";
      desc = "Edit in OnlyOffice";
      for = "linux";
    }
    {
      run = "google-chrome --new-window \"$@\"";
      desc = "Open in Google Docs/Sheets/Slides";
      for = "linux";
    }
    {
      run = "xdg-open \"$@\"";
      desc = "Open with default app";
      for = "linux";
    }
  ];
  opener.image_viewer = [
    {
      run = "loupe \"$@\"";
      desc = "Open in Loupe";
      for = "linux";
    }
    {
      run = "google-chrome \"$@\"";
      desc = "Open in Chrome";
      for = "linux";
    }
    {
      run = "gimp \"$@\"";
      desc = "Edit in GIMP";
      for = "linux";
      orphan = true;
    }
    {
      run = "xdg-open \"$@\"";
      desc = "Open with default app";
      for = "linux";
    }
  ];

  # File type associations
  open.prepend_rules = [
    # Text files - Zed
    {
      mime = "text/*";
      use = "text_editor";
    }
    {
      url = "*.{txt,md,json,yaml,yml,toml,conf,config,log,sh,py,rs,js,ts,html,css,xml,svg,nix,lua}";
      use = "text_editor";
    }

    # Media files - VLC
    {
      mime = "video/*";
      use = "media_player";
    }
    {
      mime = "audio/*";
      use = "media_player";
    }
    {
      url = "*.{mp4,mkv,avi,mov,mp3,flac,wav,ogg,m4a,webm,3gp,wmv,flv,m4v,ogv}";
      use = "media_player";
    }

    # PDF files - Chrome
    {
      mime = "application/pdf";
      use = "pdf_viewer";
    }
    {
      url = "*.pdf";
      use = "pdf_viewer";
    }

    # Images - Loupe
    {
      mime = "image/*";
      use = "image_viewer";
    }

    # Office documents - OnlyOffice
    {
      mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
      use = "office_suite";
    } # .docx
    {
      mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
      use = "office_suite";
    } # .xlsx
    {
      mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
      use = "office_suite";
    } # .pptx
    {
      mime = "application/msword";
      use = "office_suite";
    } # .doc
    {
      mime = "application/vnd.ms-powerpoint";
      use = "office_suite";
    } # .ppt
    {
      mime = "application/vnd.ms-excel";
      use = "office_suite";
    } # .xls
    {
      url = "*.{doc,docx,ppt,pptx,csv,xls,xlsx,odt,ods,odp}";
      use = "office_suite";
    }
  ];
}