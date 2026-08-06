{ pkgs, ... }:
{
  home.packages = with pkgs; [
    (symlinkJoin {
      name = "birdtray-xcb";
      paths = [ birdtray ];
      nativeBuildInputs = [ makeWrapper ];
      postBuild = ''
        wrapProgram $out/bin/birdtray --set QT_QPA_PLATFORM xcb
      '';
    })
  ];

  programs.thunderbird = {
    enable = true;

    package = pkgs.symlinkJoin {
      name = "thunderbird-xwayland-${pkgs.thunderbird.version}";
      paths = [ pkgs.thunderbird ];
      nativeBuildInputs = [ pkgs.makeWrapper ];
      inherit (pkgs.thunderbird) version meta;
      postBuild = ''
        wrapProgram $out/bin/thunderbird --set MOZ_ENABLE_WAYLAND 0
      '';
    };

    profiles.default = {
      isDefault = true;
      settings = {
        "mail.spaces.toolbar.enabled" = false;
        # Chat / instant messaging.
        "mail.chat.enabled" = false;
        # Thunderbird start page.
        "mailnews.start_page.enabled" = false;
        "mail.collect_email_address_outgoing" = false;
        "mail.collect_email_address_incoming" = false;
        # Telemetry and data submission.
        "datareporting.policy.dataSubmissionEnabled" = false;
        "toolkit.telemetry.enabled" = false;
        "toolkit.telemetry.unified" = false;
        "datareporting.healthreport.uploadEnabled" = false;
        "datareporting.healthreport.service.enabled" = false;
        "datareporting.crashreporter.uploadEnabled" = false;
        # New-account / migration wizards and hub prompts.
        "mail.provider.enabled" = false;
        # RSS / Newsgroups (we only want mail).
        "mailnews.ui.newsrc_root" = false;
        # Calendar integration notifications.
        "calendar.integration.notify" = false;
        # Disable calendar alarms (no calendar is configured anyway).
        "calendar.alarm.playsound" = false;
        "calendar.alarms.show" = false;
        # Don't auto-detect or suggest calendar providers for mail accounts.
        "calendar.provider.autoconfigure" = false;
        # Address book: don't auto-collect addresses from outgoing mail.
        "mail.collect_addressbook" = "";
        # Don't prompt to set up online directory (LDAP) accounts.
        "ldap_2.autoComplete.useDirectory" = false;
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
        # Don't show the donate / fund-raising banners.
        "app.donation.eoy.version" = 9999;
        # Don't remember passwords in Thunderbird's password manager.
        "signon.rememberSignons" = false;
      };
    };
  };
}
