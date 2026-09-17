# Applications too bespoke for a plain home.packages line: patched or
# wrapped builds (FastFlix, Krokiet, Birdtray, PDF4Qt, OnlyOffice,
# Thunderbird), VS Code and its managed settings, and the OCR screenshot
# binding. Menu entries live in desktop.nix.
{
  config,
  lib,
  pkgs,
  repoPackages,
  ...
}:

let
  # Portals are configured system-wide in ../nixos/services.nix; declaring
  # them here as well would install a second copy into the user profile.
  # Qt applications only need to be pointed at the portal for theming.
  portalizeQtPackage =
    package:
    pkgs.symlinkJoin {
      name = "${package.name}-portal";
      paths = [ package ];
      nativeBuildInputs = [ pkgs.makeWrapper ];
      postBuild = ''
        for program in "$out/bin/"*; do
          if [ -f "$program" ] && [ -x "$program" ]; then
            wrapProgram "$program" --set QT_QPA_PLATFORMTHEME xdgdesktopportal
          fi
        done
      '';
      inherit (package) meta;
    };

  # Patched to follow the portal's system theme; the fixes are commented
  # inside postPatch. The menu entry and icon live in desktop.nix.
  fastflix = portalizeQtPackage (
    pkgs.fastflix.overrideAttrs (old: {
      postPatch = (old.postPatch or "") + ''
        # Default to the system theme instead of upstream's "onyx".
        substituteInPlace fastflix/models/config.py \
          --replace-fail 'theme: str = "onyx"' 'theme: str = "system"'

        # Upstream passes argv lists through a shell, losing FFmpeg's arguments.
        for previewWindow in fastflix/widgets/windows/{crop_window,large_preview}.py; do
          substituteInPlace "$previewWindow" \
            --replace-fail 'run(thumb_command, shell=True, stderr=PIPE, stdout=PIPE)' \
              'run(thumb_command, stderr=PIPE, stdout=PIPE)'
        done

        # The stylesheet-free system theme still needs dark icons and text.
        substituteInPlace fastflix/resources.py \
          --replace-fail 'if theme.lower() in ("dark", "onyx"):' \
            'if theme.lower() in ("dark", "onyx", "system"):'
        substituteInPlace fastflix/widgets/main.py \
          --replace-fail 'self.app.fastflix.config.theme in ("dark", "onyx") else "color: black"' \
            'self.app.fastflix.config.theme in ("dark", "onyx", "system") else "color: black"'

        substituteInPlace fastflix/widgets/status_bar.py \
          --replace-fail '"#StatusBarWidget {  background-color: #f0f0f0;  border-top: 1px solid #cccccc;}"' '""' \
          --replace-fail '"color: #333333; background: transparent;"' '""'

        # Group the window with the fastflix.desktop entry in desktop.nix.
        substituteInPlace fastflix/application.py \
          --replace-fail 'main_app.setApplicationDisplayName("FastFlix")' \
            'QtGui.QGuiApplication.setDesktopFileName("fastflix"); main_app.setApplicationDisplayName("FastFlix")'
      '';
    })
  );

  # nixpkgs' czkawka builds the legacy GTK GUI (czkawka_gui) alongside
  # Krokiet, its newer GUI; strip the legacy binary and its desktop entry,
  # icons and metainfo so only Krokiet and the CLI remain.
  krokiet = pkgs.runCommand "krokiet-${pkgs.czkawka-full.version}" { } ''
    cp -rL ${pkgs.czkawka-full} $out
    chmod -R +w $out
    rm -f $out/bin/czkawka_gui
    rm -f $out/share/applications/com.github.qarmin.czkawka.desktop
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka.svg
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka-symbolic.svg
    rm -f $out/share/metainfo/com.github.qarmin.czkawka.metainfo.xml
  '';

  birdtrayXcb = pkgs.symlinkJoin {
    name = "birdtray-xcb";
    paths = [ pkgs.birdtray ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/birdtray --set QT_QPA_PLATFORM xcb
    '';
  };

  onlyofficeScaled = pkgs.symlinkJoin {
    name = "onlyoffice-desktopeditors-scaled";
    paths = [ pkgs.onlyoffice-desktopeditors ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
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

  thunderbirdXwayland = pkgs.symlinkJoin {
    name = "thunderbird-xwayland-${pkgs.thunderbird.version}";
    paths = [ pkgs.thunderbird ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    inherit (pkgs.thunderbird) version meta;
    postBuild = ''
      wrapProgram $out/bin/thunderbird --set MOZ_ENABLE_WAYLAND 0
    '';
  };

  # Home Manager symlinks userSettings into the store, leaving settings.json
  # read-only so VS Code's settings UI cannot write. Merging into a real file
  # during activation keeps it writable: the keys below still win on every
  # switch, and anything VS Code writes alongside them survives.
  vscodeUserSettings = {
    "workbench.colorTheme" = "Daemon-2.0";
    "chat.disableAIFeatures" = true;
    "chat.commandCenter.enabled" = false;
    "editor.inlineSuggest.enabled" = false;

    # A scrollbar's thickness is a setting, not a theme colour: the defaults
    # are 14 and 12 pixels, sized for a slider the eye has to hunt for. This
    # one is solid Daemon yellow, so a third of that is plenty to grab.
    "editor.scrollbar.verticalScrollbarSize" = 5;
    "editor.scrollbar.horizontalScrollbarSize" = 5;
  };

  vscodeUserSettingsFile =
    (pkgs.formats.json { }).generate "vscode-user-settings.json"
      vscodeUserSettings;

  # The same location Home Manager's VS Code module uses for the default
  # profile of programs.vscode.package (pkgs.vscode).
  vscodeUserSettingsPath = "${config.xdg.configHome}/Code/User/settings.json";
in
{
  # ── Packages ──────────────────────────────────────────────────────────
  home.packages = [
    birdtrayXcb
    fastflix
    krokiet
    (portalizeQtPackage pkgs.pdf4qt)
    repoPackages.ocrScreenshot
  ];

  # ── OCR screenshot ────────────────────────────────────────────────────
  dconf.settings = {
    "org/gnome/settings-daemon/plugins/media-keys" = {
      custom-keybindings = [
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut/"
      ];
    };
    "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut" = {
      binding = "<Shift>Print";
      command = "${repoPackages.ocrScreenshot}/bin/ocr-screenshot";
      name = "OCR Screenshot";
    };
  };

  # ── OnlyOffice ────────────────────────────────────────────────────────
  programs.onlyoffice = {
    enable = true;
    package = onlyofficeScaled;
  };

  home.activation.onlyofficeFonts = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu

    sourceDir="/run/current-system/sw/share/X11/fonts"
    destination="${config.xdg.dataHome}/fonts/onlyoffice"
    fontCache="${config.xdg.dataHome}/onlyoffice/desktopeditors/data/fonts"

    if [ ! -d "$sourceDir" ]; then
      echo "OnlyOffice: $sourceDir is missing; enable fonts.fontDir.enable"
    else
      mkdir -p "$destination"

      # -L dereferences NixOS font symlinks into real files.
      ${pkgs.rsync}/bin/rsync -aL --delete "$sourceDir/" "$destination/"

      ${pkgs.findutils}/bin/find "$destination" -type d -exec chmod 0755 {} +
      ${pkgs.findutils}/bin/find "$destination" -type f -exec chmod 0644 {} +

      # Force OnlyOffice to rebuild its internal font cache.
      if [ -d "$fontCache" ]; then
        ${pkgs.findutils}/bin/find "$fontCache" -mindepth 1 -delete
      fi

      ${pkgs.fontconfig}/bin/fc-cache -f "$destination" >/dev/null 2>&1 || true
    fi
  '';

  # ── VS Code ─────────────────────────────────────────────────────
  programs.vscode.enable = true;

  # Runs after linkGeneration so Home Manager has already cleaned up the
  # symlink an earlier generation left at this path.
  home.activation.vscodeUserSettings = lib.hm.dag.entryAfter [ "writeBoundary" "linkGeneration" ] ''
    set -eu

    settings="${vscodeUserSettingsPath}"
    managed="${vscodeUserSettingsFile}"

    # Staged in the target's directory so each update lands as a rename.
    # settings.json is then never absent or half written: a running VS Code
    # that catches it missing loads an empty model and writes it back,
    # dropping everything the file held.
    staging="$(dirname "$settings")/.settings.json.hm-new"

    mkdir -p "$(dirname "$settings")"

    if [ -L "$settings" ] || [ ! -s "$settings" ]; then
      # Either the read-only symlink from an earlier generation or no
      # settings worth keeping, so the declared ones simply replace it.
      ${pkgs.coreutils}/bin/install -m 0644 "$managed" "$staging"
      mv -f "$staging" "$settings"
    elif ! ${pkgs.jq}/bin/jq -e 'type == "object"' "$settings" >/dev/null 2>&1; then
      # VS Code accepts comments in settings.json and jq does not, so a
      # file jq cannot read is not necessarily broken. Rewriting it would
      # throw away real settings, so say something and leave it alone.
      echo "VS Code: $settings is not a JSON object; leaving it untouched"
    elif ${pkgs.jq}/bin/jq -e -s '.[0] * .[1] == .[0]' \
      "$settings" "$managed" >/dev/null; then
      # Every declared key already holds its declared value. Writing now
      # would only reformat what VS Code wrote, so leave the file alone.
      :
    else
      # jq's * merges recursively with the right-hand side winning, so the
      # settings declared above are restored while everything VS Code
      # added on its own is carried over.
      ${pkgs.jq}/bin/jq -s '.[0] * .[1]' "$settings" "$managed" > "$staging"
      chmod 0644 "$staging"
      mv -f "$staging" "$settings"
    fi
  '';

  # ── Thunderbird ───────────────────────────────────────────────────────
  programs.thunderbird = {
    enable = true;
    package = thunderbirdXwayland;

    profiles.default = {
      isDefault = true;

      # The fontconfig sans default is monospace (../nixos/default.nix), so
      # Gecko sizes dialogs for text that then wraps an extra line and pushes
      # the buttons past the bottom edge; on XWayland the window cannot grow
      # to fit. Chrome documents only — message bodies keep their own fonts.
      userChrome = ''
        * {
          font-family: "Noto Sans", "Noto Sans CJK SC", sans-serif !important;
        }
      '';

      settings = {
        # Required for the userChrome.css above to be loaded at all.
        "toolkit.legacyUserProfileCustomizations.stylesheets" = true;

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
