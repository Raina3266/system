{ pkgs, ... }:
{
  home.packages = with pkgs; [
    # Birdtray under X11 (xwayland-satellite): Qt5's Wayland platform
    # plugin never reports a system tray as available on niri, so
    # birdtray's QSystemTrayIcon::isSystemTrayAvailable() loop times out
    # after 60s with "system tray cannot be controlled". The xcb backend
    # detects waybar's StatusNotifierWatcher via XWayland just fine.
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

    # Birdtray (forced to xcb above) decides whether Thunderbird is running
    # solely by looking up its X11 window -- its log prints "Window ID found:
    # 0" when the lookup fails, which is what draws the red cross. A
    # native-Wayland Thunderbird owns no X11 window, so the lookup always
    # failed; combined with common/launchthunderbird=true, birdtray also read
    # every close as a crash and immediately relaunched it. Running
    # Thunderbird on XWayland makes the window visible to birdtray and fixes
    # both symptoms. Trade-off: blurrier rendering under fractional scaling.
    package = pkgs.symlinkJoin {
      name = "thunderbird-xwayland-${pkgs.thunderbird.version}";
      paths = [ pkgs.thunderbird ];
      nativeBuildInputs = [ pkgs.makeWrapper ];
      # Home Manager reads .version off this package to derive
      # programs.thunderbird.release, and symlinkJoin would otherwise drop it.
      inherit (pkgs.thunderbird) version meta;
      postBuild = ''
        wrapProgram $out/bin/thunderbird --set MOZ_ENABLE_WAYLAND 0
      '';
    };

    profiles.default = {
      isDefault = true;
      settings = {
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
        # Disable calendar alarms (no calendar is configured anyway).
        "calendar.alarm.show" = false;
        "calendar.alarm.playsound" = false;
        # Don't auto-detect or suggest calendar providers for mail accounts.
        "calendar.provider.autoconfigure" = false;
        # Address book: don't auto-collect addresses from outgoing mail.
        "mail.collect_email_address_outgoing" = false;
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
