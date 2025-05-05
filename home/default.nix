{
  lib,
  config,
  pkgs,
  ...
}: {
  imports = [
    ./cloud.nix
    ./ocr.nix
    ./vscode.nix
    ./shell/default.nix
  ];

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.05";
  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  home.packages = with pkgs; [
    evolution
    evolution-ews
    google-chrome
    whatsapp-for-linux
    discord
    obsidian
    vlc
    gimp-with-plugins
    kdePackages.kdenlive
    obs-studio
    qbittorrent
    anki-bin
    libreoffice
    kid3
    strawberry
    yt-dlp
    spotdl
  ];

  xdg.enable = true;

  # trick to force gnome to use proper icons for apps installed through nix
  #  xdg.systemDirs.data = ["~/.nix-profile/share"];

  # home.file.".local/share/applications/libreoffice-writer.desktop".text = ''
  #   NoDisplay = true
  #   Name = LibreOffice Writer
  #   Exec = libreoffice --writer
  #   Hidden = true
  #   Type = Application
  # '';
}
