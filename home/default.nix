{
  pkgs,
  inputs,
  ...
}:
{
  imports = [
    ./niri
    ./shell
    ./cloud.nix
    ./ocr.nix
    ./toolchains.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs.home-manager.enable = true;

  programs.zed-editor = {
    enable = true;
    # Nightly from the upstream flake (matches zed.cachix.org; see flake.nix).
    package = inputs.zed.packages.${pkgs.stdenv.hostPlatform.system}.default;
    userSettings = {
      terminal.shell.program = "fish";
      theme = {
        mode = "dark";
        dark = "Gruvbox Dark Hard";
      };
    };
  };

  programs.thunderbird = {
    enable = true;
    profiles.default = {
      isDefault = true;
      settings = {
        # ── Disable features I don't use ─────────────────────────────
        # Chat / instant messaging.
        "mail.chat.enabled" = false;
        # Thunderbird start page.
        "mailnews.start_page.enabled" = false;
        # Telemetry and data submission.
        "datareporting.policy.dataSubmissionEnabled" = false;
        "toolkit.telemetry.enabled" = false;
        "toolkit.telemetry.unified" = false;
        "datareporting.healthreport.uploadEnabled" = false;
        "datareporting.healthreport.service.enabled" = false;
        "datareporting.crashreporter.uploadEnabled" = false;
        # New-account / migration wizards and hub prompts.
        "mail.provider.enabled" = false;
        "mailnews.ui.account_settings_page" = false;
        # RSS / Newsgroups (we only want mail).
        "mailnews.ui.newsrc_root" = false;
        # Calendar integration notifications.
        "calendar.integration.notify" = false;
        # Add-on recommendations and discovery pane.
        "extensions.getAddons.showPane" = false;
        "extensions.ui.lastCategory" = "addons://list/extension";
        # Pocket (Mozilla's read-it-later service).
        "extensions.pocket.enabled" = false;

        # ── Quiet the UI ─────────────────────────────────────────────
        # Don't show the account central / welcome page per folder.
        "mailnews.start_page_override.mstone" = "ignore";
        # Disable the "What's New" tab after upgrades.
        "app.update.showInstalledUI" = false;
        # Don't prompt to set Thunderbird as the default mail client.
        "mail.shell.checkDefaultClient" = false;
        # Hide the folder-pane account central items (RSS, newsgroups, chat).
        "mail.folderpane.mode" = "compact";
        # Disable the new mail toast notifications (we keep the badge).
        "mail.biff.show_alert" = false;
        # Don't show the donate / fund-raising banners.
        "app.donation.eoy.version" = 9999;
        # Don't remember passwords in Thunderbird's password manager.
        "signon.rememberSignons" = false;
      };
    };
  };

  home.packages = with pkgs; [
    # browsers
    google-chrome
    firefox

    # communication
    slack
    discord
    zoom-us
    wechat
    qq
    wemeet
    whatsie

    # productivity / office
    libreoffice
    obsidian
    anki
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    vlc
    tauon
    waylyrics

    # media creation / editing
    pavucontrol
    obs-studio
    inkscape
    shotcut
    sunshine
    kid3
    spotdl
    yt-dlp

    # media servers / sync
    jellyfin
    jellyfin-web
    immich

    # downloads / torrent
    qbittorrent
    clash-verge-rev
  ];
}
