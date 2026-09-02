{
  config,
  lib,
  pkgs,
  ...
}:

let
  onlyofficeFonts = "${config.xdg.dataHome}/fonts/onlyoffice";

  # Home Manager writes programs.vscode.profiles.<name>.userSettings as a
  # symlink into the store, so settings.json ends up read-only and VS Code's
  # own settings UI fails with "Unable to write into user settings". These
  # settings are merged into a real file during activation instead, which
  # leaves the file writable. The keys below still win on every switch;
  # anything VS Code writes alongside them is preserved.
  vscodeUserSettings = {
    "workbench.colorTheme" = "Daemon-2.0";
    "chat.disableAIFeatures" = true;
    "chat.commandCenter.enabled" = false;
    "editor.inlineSuggest.enabled" = false;
  };

  vscodeUserSettingsFile =
    (pkgs.formats.json { }).generate "vscode-user-settings.json" vscodeUserSettings;

  # The same location Home Manager's VS Code module uses for the default
  # profile of programs.vscode.package (pkgs.vscode).
  vscodeUserSettingsPath = "${config.xdg.configHome}/Code/User/settings.json";

  onlyofficeScaled = pkgs.symlinkJoin {
    name = "onlyoffice-desktopeditors-scaled";

    paths = [
      pkgs.onlyoffice-desktopeditors
    ];

    nativeBuildInputs = [
      pkgs.makeWrapper
    ];

    postBuild = ''
      rm "$out/bin/onlyoffice-desktopeditors"

      makeWrapper \
        "${pkgs.onlyoffice-desktopeditors}/bin/onlyoffice-desktopeditors" \
        "$out/bin/onlyoffice-desktopeditors" \
        --unset QT_SCALE_FACTOR \
        --unset QT_SCREEN_SCALE_FACTORS \
        --unset QT_AUTO_SCREEN_SCALE_FACTOR \
        --add-flags "--force-scale=1"
    '';
  };
in
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

  programs.onlyoffice = {
    enable = true;
    package = onlyofficeScaled;
  };

  home.activation.onlyofficeFonts = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu

    sourceDir="/run/current-system/sw/share/X11/fonts"
    destination="${onlyofficeFonts}"
    fontCache="${config.xdg.dataHome}/onlyoffice/desktopeditors/data/fonts"

    if [ ! -d "$sourceDir" ]; then
      echo "OnlyOffice: $sourceDir is missing; enable fonts.fontDir.enable"
    else
      mkdir -p "$destination"

      # -L dereferences NixOS font symlinks into real files.
      ${pkgs.rsync}/bin/rsync \
        -aL \
        --delete \
        "$sourceDir/" \
        "$destination/"

      ${pkgs.findutils}/bin/find "$destination" \
        -type d -exec chmod 0755 {} +

      ${pkgs.findutils}/bin/find "$destination" \
        -type f -exec chmod 0644 {} +

      # Force OnlyOffice to rebuild its internal font cache.
      if [ -d "$fontCache" ]; then
        ${pkgs.findutils}/bin/find "$fontCache" \
          -mindepth 1 -delete
      fi

      ${pkgs.fontconfig}/bin/fc-cache \
        -f "$destination" >/dev/null 2>&1 || true
      fi
  '';

  # Runs after linkGeneration so the store symlink left by an earlier
  # generation is already gone by the time the writable file is placed.
  home.activation.vscodeUserSettings =
    lib.hm.dag.entryAfter [ "writeBoundary" "linkGeneration" ]
      ''
        set -eu

        settings="${vscodeUserSettingsPath}"
        managed="${vscodeUserSettingsFile}"

        mkdir -p "$(dirname "$settings")"

        # Switching to this arrangement from a generation that still managed
        # the file leaves the read-only symlink behind; it has to go before a
        # writable file can take its place.
        if [ -L "$settings" ]; then
          rm -f "$settings"
        fi

        # -s also covers the file being absent, and an empty settings.json
        # is what VS Code leaves behind often enough to treat the same way.
        if [ ! -s "$settings" ]; then
          ${pkgs.coreutils}/bin/install -m 0644 "$managed" "$settings"
        elif ${pkgs.jq}/bin/jq -e 'type == "object"' "$settings" >/dev/null 2>&1; then
          # jq's * merges recursively with the right-hand side winning, so the
          # settings declared above are reapplied while everything VS Code
          # added on its own is carried over.
          ${pkgs.jq}/bin/jq -s '.[0] * .[1]' "$settings" "$managed" \
            > "$settings.hm-new"
          mv "$settings.hm-new" "$settings"
          chmod 0644 "$settings"
        else
          # VS Code accepts comments in settings.json and jq does not, so a
          # file jq cannot read is not necessarily broken. Rewriting it would
          # throw away real settings, so say something and leave it alone.
          echo "VS Code: $settings is not a JSON object; leaving it untouched"
        fi
      '';

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
        "mail.chat.enabled" = false;
        "mailnews.start_page.enabled" = false;
        "mail.collect_email_address_outgoing" = false;
        "mail.collect_email_address_incoming" = false;
        "datareporting.policy.dataSubmissionEnabled" = false;
        "toolkit.telemetry.enabled" = false;
        "toolkit.telemetry.unified" = false;
        "datareporting.healthreport.uploadEnabled" = false;
        "datareporting.healthreport.service.enabled" = false;
        "datareporting.crashreporter.uploadEnabled" = false;
        "mail.provider.enabled" = false;
        "mailnews.ui.newsrc_root" = false;
        "calendar.integration.notify" = false;
        "calendar.alarm.playsound" = false;
        "calendar.alarms.show" = false;
        "calendar.provider.autoconfigure" = false;
        "mail.collect_addressbook" = "";
        "ldap_2.autoComplete.useDirectory" = false;
        "extensions.getAddons.showPane" = false;
        "extensions.ui.lastCategory" = "addons://list/extension";
        "extensions.pocket.enabled" = false;
        "mailnews.start_page_override.mstone" = "ignore";
        "app.update.showInstalledUI" = false;
        "mail.shell.checkDefaultClient" = false;
        "mail.folderpane.mode" = "compact";
        "app.donation.eoy.version" = 9999;
        "signon.rememberSignons" = false;
      };
    };
  };
}
