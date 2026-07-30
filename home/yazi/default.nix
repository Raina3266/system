# Yazi file manager.
{ lib, pkgs, ... }:
let
  # Pinned to a concrete commit (not the moving "main" branch) so the
  # hash stays stable. Bump rev + hash together when updating.
  yazi-plugins-repo = pkgs.fetchFromGitHub {
    owner = "yazi-rs";
    repo = "plugins";
    rev = "9014ed21f3a62c71907751e8dd5b9f4882124b74";
    sha256 = "sha256-HUbc5JJwBznvyOoZnQVq18K991LQ+ksCGXN0Gj7GQjE=";
  };
  batch-rename-gui = "${yazi-plugins-repo}/batch-rename-gui.yazi";
in
{
  programs.yazi = {
    enable = true;
    enableFishIntegration = true;
    enableBashIntegration = true;

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
    ] pkgs.yaziPlugins) // {
      # Custom plugins not in nixpkgs
      batch-rename-gui = batch-rename-gui;
    };

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
