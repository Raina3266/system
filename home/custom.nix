# Applications too bespoke for a plain home.packages line: patched or
# wrapped builds, and the OCR screenshot binding. Menu entries live in
# desktop.nix.
{
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
      # The crop window renders its previews as TIFFs, but Qt's TIFF image
      # plugin ships in qtimageformats, which the nixpkgs expression does
      # not depend on. Without it every preview loads as a null pixmap and
      # the crop UI silently stops working (JPEG thumbnails are unaffected,
      # as their plugin lives in qtbase). wrapQtAppsHook picks this up and
      # adds the plugin directory to QT_PLUGIN_PATH.
      buildInputs = (old.buildInputs or [ ]) ++ [ pkgs.qt6.qtimageformats ];

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

  # PDF4Qt builds its whole suite in one derivation; keep only Editor and
  # PageMaster by stripping the other apps' binaries, desktop entries,
  # icons and metainfo. PdfTool, the suite's CLI, stays.
  pdf4qt = pkgs.runCommand "pdf4qt-${pkgs.pdf4qt.version}" { } ''
    cp -rL ${pkgs.pdf4qt} $out
    chmod -R +w $out
    rm -f $out/bin/Pdf4Qt{Diff,LaunchPad,Viewer}
    rm -f $out/share/applications/io.github.JakubMelka.Pdf4qt{,.Pdf4QtDiff,.Pdf4QtViewer}.desktop
    rm -f $out/share/icons/hicolor/*/apps/io.github.JakubMelka.Pdf4qt{,.Pdf4QtDiff,.Pdf4QtViewer}.{png,svg}
    rm -f $out/share/metainfo/io.github.JakubMelka.Pdf4qt.appdata.xml
  '';

in
{
  # ── Packages ──────────────────────────────────────────────────────────
  home.packages = [
    fastflix
    krokiet
    (portalizeQtPackage pdf4qt)
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

  # ── Thunderbird ───────────────────────────────────────────────────────
  programs.thunderbird = {
    enable = true;

    profiles.default = {
      isDefault = true;
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
