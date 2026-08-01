# Yazi core settings: manager, plugin fetchers/previewers and openers.
let
  # openers all fall back to xdg-open as a last resort
  xdgOpen = { run = "xdg-open \"$@\""; desc = "Open with default app"; for = "linux"; };
in
{
  mgr = {
    show_hidden = true;
    sort_by = "alphabetical";
    sort_dir_first = true;
    linemode = "size";
    show_symlink = true;
    ratio = [ 3 3 3 ]; # parent / current / preview pane widths
  };

  plugin = {
    prepend_fetchers = [
      # Extension-based mime detection. `group = "mime"` makes these replace the
      # built-in fetcher (only the first match in a group runs).
      { "if" = "!mime"; url = "local://*"; run = "mime-ext.local"; prio = "high"; group = "mime"; }
      { "if" = "!mime"; url = "remote://*"; run = "mime-ext.remote"; prio = "high"; group = "mime"; }
      # Git status, feeds the `git` linemode + githead.
      { url = "*"; run = "git"; group = "git"; }
      { url = "*/"; run = "git"; group = "git"; }
    ];

    prepend_preloaders = [
      # allmytoes: freedesktop thumbnails for images (magick for types it can't)
      { mime = "image/svg+xml"; run = "magick"; }
      { mime = "image/heic"; run = "magick"; }
      { mime = "image/jxl"; run = "magick"; }
      { mime = "image/*"; run = "allmytoes"; }

      # mediainfo: media thumbnail + metadata (replaces built-in audio/video previewer)
      { mime = "{audio,video}/*"; run = "mediainfo"; }
      { mime = "application/{subrip,postscript,illustrator,dvb.ait,vnd.adobe.illustrator,eps}"; run = "mediainfo"; }
      { url = "*.{ai,eps,ait}"; run = "mediainfo"; }
    ];

    prepend_previewers = [
      { url = "*.md"; run = "rich-preview"; }
      { url = "*.rst"; run = "rich-preview"; }
      { url = "*.ipynb"; run = "rich-preview"; }
      { url = "*.csv"; run = "rich-preview"; }
      { url = "*.json"; run = "rich-preview"; }

      { mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"; run = "office"; }
      { mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"; run = "office"; }
      { mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation"; run = "office"; }

      { mime = "application/{,g}zip"; run = "lsar"; }
      { mime = "application/x-{tar,bzip2,7z-compressed,xz,rar}"; run = "lsar"; }

      # pdf: built-in previewer, bridges to pdftoppm (poppler-utils)
      { mime = "application/pdf"; run = "pdf"; }
      { url = "*.pdf"; run = "pdf"; }

      { mime = "application/vnd.ms-excel"; run = "duckdb"; }
      { url = "*.parquet"; run = "duckdb"; }

      # allmytoes: freedesktop thumbnails for images
      { mime = "image/svg+xml"; run = "magick"; }
      { mime = "image/heic"; run = "magick"; }
      { mime = "image/jxl"; run = "magick"; }
      { mime = "image/*"; run = "allmytoes"; }

      # mediainfo: media metadata + thumbnail (audio/video only; images handled by allmytoes)
      { mime = "{audio,video}/*"; run = "mediainfo"; }
      { mime = "application/{subrip,postscript,illustrator,dvb.ait,vnd.adobe.illustrator,eps}"; run = "mediainfo"; }
      { url = "*.{ai,eps,ait}"; run = "mediainfo"; }

      # piper: fallback previewer for anything else (uses bat for text files)
      { url = "*"; run = "piper -- bat -p --color=always \"$1\""; }
    ];
  };

  # First entry of each opener is the default used by plain `open`; the rest
  # only appear in the "Open with..." (<Enter>) menu.
  opener = {
    text_editor = [
      { run = "zeditor \"$@\""; desc = "Edit in Zed"; for = "linux"; orphan = true; }
      { run = "nvim \"$@\""; desc = "Edit in Neovim"; for = "linux"; block = true; }
      xdgOpen
    ];
    media_player = [
      { run = "vlc \"$@\""; desc = "Play in VLC"; for = "linux"; }
      { run = "kid3 \"$@\""; desc = "Edit tags in Kid3"; for = "linux"; }
      xdgOpen
    ];
    pdf_viewer = [
      { run = "google-chrome --new-window \"$@\""; desc = "Open in Chrome (New Window)"; for = "linux"; }
      { run = "stirling-pdf \"$@\""; desc = "Open in Stirling PDF"; for = "linux"; }
      xdgOpen
    ];
    office_suite = [
      { run = "onlyoffice-desktopeditors \"$@\""; desc = "Edit in OnlyOffice"; for = "linux"; }
      { run = "google-chrome --new-window \"$@\""; desc = "Open in Google Docs/Sheets/Slides"; for = "linux"; }
      xdgOpen
    ];
    image_viewer = [
      { run = "loupe \"$@\""; desc = "Open in Loupe"; for = "linux"; }
      { run = "google-chrome \"$@\""; desc = "Open in Chrome"; for = "linux"; orphan = true; }
      { run = "gimp \"$@\""; desc = "Edit in GIMP"; for = "linux"; orphan = true; }
      xdgOpen
    ];
  };

  open.prepend_rules = [
    { mime = "text/*"; use = "text_editor"; }
    { url = "*.{txt,md,json,yaml,yml,toml,conf,config,log,sh,py,rs,js,ts,html,css,xml,svg,nix,lua}"; use = "text_editor"; }

    { mime = "video/*"; use = "media_player"; }
    { mime = "audio/*"; use = "media_player"; }
    { url = "*.{mp4,mkv,avi,mov,mp3,flac,wav,ogg,m4a,webm,3gp,wmv,flv,m4v,ogv}"; use = "media_player"; }

    { mime = "application/pdf"; use = "pdf_viewer"; }
    { url = "*.pdf"; use = "pdf_viewer"; }

    { mime = "image/*"; use = "image_viewer"; }

    { mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"; use = "office_suite"; } # .docx
    { mime = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"; use = "office_suite"; } # .xlsx
    { mime = "application/vnd.openxmlformats-officedocument.presentationml.presentation"; use = "office_suite"; } # .pptx
    { mime = "application/msword"; use = "office_suite"; } # .doc
    { mime = "application/vnd.ms-powerpoint"; use = "office_suite"; } # .ppt
    { mime = "application/vnd.ms-excel"; use = "office_suite"; } # .xls
    { url = "*.{doc,docx,ppt,pptx,csv,xls,xlsx,odt,ods,odp}"; use = "office_suite"; }
  ];
}
