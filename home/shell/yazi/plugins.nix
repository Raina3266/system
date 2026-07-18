# Yazi plugin configuration.
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

  # Patch glow: nixpkgs 2025-06-13 uses :args() but yazi 26.5.6 only has :arg()
  glow-patched = pkgs.runCommand "glow.yazi-patched" { } ''
    cp -r ${yp.glow} $out
    chmod -R u+w $out
    substituteInPlace $out/main.lua --replace-fail ':args(' ':arg('
  '';
in
{
  # ──────────────────────────────────────────────────────────────────────
  # Plugins — each entry is linked to $XDG_CONFIG_HOME/yazi/plugins/<name>.yazi
  # ──────────────────────────────────────────────────────────────────────
  plugins = {
    # 1. Advanced & Specialized Previews
    "glow" = glow-patched; # markdown rendering (patched: :args→:arg for yazi 26.5.6)
    "mime-ext" = yp.mime-ext; # fast mime detection by extension
    "rich-preview" = yp.rich-preview; # rich previews for various types

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
  # Tools yazi can shell out to for previews / file ops.
  # ──────────────────────────────────────────────────────────────────────
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
    rich-cli # markdown/json/csv rendering (used by rich-preview plugin)
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
}