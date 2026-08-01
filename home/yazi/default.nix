# Yazi file manager.
{ lib, pkgs, ... }:
{
  programs.yazi = {
    enable = true;
    enableFishIntegration = true;

    initLua = ./main.lua;
    settings = import ./settings.nix;
    theme = import ./theme.nix;
    keymap = import ./keymap.nix;

    # Linked into $XDG_CONFIG_HOME/yazi/plugins/<name>.yazi. Taken from the
    # yaziPlugins bundle so versions always match the yazi package.
    # Anything used from main.lua, keymap.nix or settings.nix must be listed here.
    plugins = (lib.getAttrs [
      # Previewers
      "mime-ext" # fast mime detection by extension
      "rich-preview" # markdown/csv/json/ipynb/rst
      "office" # .docx/.xlsx/.pptx
      "lsar" # archive contents listing
      "duckdb" # csv/parquet/xls via SQL
      "mediainfo" # media metadata + thumbnails
      "allmytoes" # freedesktop thumbnails for images
      "piper" # pipe any shell command as previewer
      "convert" # image format conversion

      # File manipulation
      "smart-filter"
      "smart-enter"
      "smart-paste"
      "bookmarks"
      "ouch" # archive compress/extract
      "compress" # archive creation
      "recycle-bin" # trash management
      "chmod"
      "mount" # disk mount/unmount/eject
      "rsync"
      "sudo"
      "diff"

      # UI & navigation
      "full-border"
      "yatline" # custom header/status lines
      "starship" # starship prompt in header
      "git" # git status in listings
      "githead" # git branch in header
      "toggle-pane"
      "zoom"
      "jump-to-char"
      "easyjump"
      "yafg" # ripgrep+fzf content search
    ] pkgs.yaziPlugins);

    # Tools yazi shells out to for previews and file ops.
    extraPackages = with pkgs; [
      bat # syntax highlighting
      chafa # terminal image fallback
      catdoc # .doc
      duckdb # duckdb plugin
      exiftool
      fd
      ffmpeg
      fzf # yafg plugin
      fontpreview
      imagemagick
      jq
      mediainfo
      poppler-utils # pdftoppm
      rich-cli # rich-preview plugin
      ripgrep
      unar # lsar plugin, archive extract
      xlsx2csv
      zoxide

      allmytoes # allmytoes plugin
      ouch # ouch plugin
      rsync # rsync plugin
      trash-cli # recycle-bin plugin
      zip # compress plugin
      p7zip # compress plugin (.7z, encrypted zip)
      util-linux # mount plugin (lsblk, eject)
      udisks # mount plugin (udisksctl)
    ];
  };
}
