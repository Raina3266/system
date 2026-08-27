# Every theme this configuration applies, in one module.
#
# Two mechanisms live here. The stylesheets in this repository are linked into
# place with mkOutOfStoreSymlink, so they stay outside the Nix store and an
# edit is picked up the next time the program starts without a Home Manager
# rebuild. The Daemon KDE MK2 theme comes from a pinned upstream checkout and
# is copied in from the store, plus the activation step that selects it. Its
# VS Code extension is repackaged from that same checkout and handed to the
# programs.vscode module.
{
  config,
  pkgs,
  ...
}:
let
  # --- Repository stylesheets, linked out of the store -----------------------

  repoRoot = "${config.home.homeDirectory}/System";

  link = path: { source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}"; };

  # <path under $XDG_CONFIG_HOME> = <path in this repository>
  configLinks = {
    "gtk-3.0/gtk.css" = "themes/gtk3.css"; # GTK3 apps (pavucontrol, file dialogs)
    "gtk-4.0/gtk.css" = "themes/gtk4.css"; # GTK4 apps (portal file chooser, image viewer)
    "preview-panel/preview-panel.css" = "themes/preview-panel.css";
    "rofi/config.rasi" = "niri/rofi/config.rasi";
    "rofi/media-control.rasi" = "themes/media-control.rasi";
    "rofi/rofi-audio.rasi" = "themes/rofi-audio.rasi";
    "rofi/rofi-clipboard.rasi" = "themes/rofi-clipboard.rasi";
    "rofi/rofi-finder.rasi" = "themes/rofi-finder.rasi";
    "rofi/rofi-network.rasi" = "themes/rofi-network.rasi";
    "waybar/style.css" = "themes/waybar.css";
    "yazi/theme.toml" = "themes/yazi.toml";
  };

  # <path under $XDG_DATA_HOME> = <path in this repository>
  dataLinks = {
    "color-schemes/Cyberpunk.colors" = "themes/Cyberpunk.colors"; # hand-written, edit in place
    "fcitx5/themes/cyberpunk/theme.conf" = "themes/fcitx5.conf";
  };

  # --- Daemon KDE MK2, from upstream ----------------------------------------

  # Pin the upstream theme so a rebuild cannot silently change its appearance.
  daemonTheme = builtins.fetchGit {
    url = "https://github.com/MathisP75/daemon-kde-mk2.git";
    rev = "01bf4df4666e9021ac8013bc2c4eaabc8d312d68";
  };
  daemonRed = "#D52C35";
  daemonPink = "#D656C7";
  daemonDimPink = "#FCEE0A";
  daemonAlternateBackground = "#210E15";
  daemonBackground = "#180A10";
  daemonSecondaryBackground = "#0F0C17";
  daemonChromeBackground = "#170A0F";

  # Reuse the same local palette across KDE and VS Code: relevant icons use
  # vivid dark red, interactive outlines use cyberpunk pink, their backgrounds
  # use dimmed pink, and normal surfaces use darker variants of Daemon's
  # burgundy palette. The deepest background, text and unrelated icon
  # categories stay intact.
  daemonPatched =
    pkgs.runCommandLocal "daemon-2.0-patched" { nativeBuildInputs = [ pkgs.python3 ]; }
      ''
        python3 ${./patch-daemon.py} desktop \
          --source ${daemonTheme} \
          --out "$out" \
          --icon-colour "${daemonRed}" \
          --pink "${daemonPink}" \
          --dim-pink "${daemonDimPink}" \
          --alternate-background "${daemonAlternateBackground}" \
          --main-background "${daemonBackground}" \
          --secondary-background "${daemonSecondaryBackground}" \
          --chrome-background "${daemonChromeBackground}"
      '';

  # Upstream ships the VS Code theme as a plain extension directory rather
  # than a marketplace package, so repackage it into the layout Home Manager
  # expects (share/vscode/extensions/<unique id>). Upstream's package.json has
  # no publisher field, which would leave VS Code calling the extension
  # "undefined_publisher.daemon-2-0"; add one so the identifier matches the
  # directory name.
  daemonVscodeSrc = "${daemonTheme}/VSCode/daemon-2-0";
  daemonVscodeManifest = builtins.fromJSON (builtins.readFile "${daemonVscodeSrc}/package.json");
  daemonVscodePublisher = "MathisP75";
  daemonVscodeId = "${daemonVscodePublisher}.${daemonVscodeManifest.name}";
  dracula = pkgs.vscode-extensions.dracula-theme.theme-dracula;
  draculaVscodeSrc = "${dracula}/share/vscode/extensions/${dracula.vscodeExtUniqueId}";

  daemonVscodeTheme =
    pkgs.runCommandLocal "vscode-extension-${daemonVscodeManifest.name}"
      {
        nativeBuildInputs = [
          pkgs.jq
          pkgs.python3
        ];
        # Home Manager takes the extension's identity from these rather than
        # listing the built directory, which would be an import-from-derivation
        # on every evaluation. It writes all of them into extensions.json, so
        # leaving any out is an evaluation error rather than a silent default.
        passthru = {
          inherit (daemonVscodeManifest) version;
          vscodeExtPublisher = daemonVscodePublisher;
          vscodeExtName = daemonVscodeManifest.name;
          vscodeExtUniqueId = daemonVscodeId;
        };
      }
      ''
        dir="$out/share/vscode/extensions/${daemonVscodeId}"
        mkdir -p "$dir/themes"

        jq --arg publisher "${daemonVscodePublisher}" \
          '. + { publisher: $publisher }' \
          ${daemonVscodeSrc}/package.json > "$dir/package.json"

        cp ${daemonVscodeSrc}/themes/*.json "$dir/themes/"

        daemon_theme="$(jq -r '.contributes.themes[0].path' "$dir/package.json")"
        dracula_theme="$(jq -r \
          '.contributes.themes[] | select(.label == "Dracula Theme") | .path' \
          ${draculaVscodeSrc}/package.json)"

        if [ -z "$dracula_theme" ]; then
          echo "Dracula Theme was not found in ${draculaVscodeSrc}/package.json" >&2
          exit 1
        fi

        python3 ${./patch-daemon.py} vscode \
          --daemon-theme "$dir/$daemon_theme" \
          --dracula-theme "${draculaVscodeSrc}/$dracula_theme" \
          --out "$dir/$daemon_theme" \
          --name "Daemon-2.0" \
          --icon-colour "${daemonRed}" \
          --pink "${daemonPink}" \
          --dim-pink "${daemonDimPink}" \
          --main-background "${daemonBackground}" \
          --secondary-background "${daemonSecondaryBackground}" \
          --chrome-background "${daemonChromeBackground}"
      '';

  kwriteconfig = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6";
  kdeConfigHome = config.xdg.configHome;

  # Both Kvantum packages ship a set of Kv* colour schemes in share/color-schemes
  # alongside the style plugin, which is all this configuration wants from them.
  # Drop the schemes so System Settings -> Colours offers Daemon2 alone. The
  # style plugin itself lives under lib/ and is untouched.
  withoutColorSchemes =
    package:
    package.overrideAttrs (old: {
      postInstall = (old.postInstall or "") + ''
        rm -rf "$out/share/color-schemes"
      '';
    });
in
{
  home.packages = with pkgs; [
    (withoutColorSchemes libsForQt5.qtstyleplugin-kvantum)
    (withoutColorSchemes qt6Packages.qtstyleplugin-kvantum)

    # Daemon-Icons declares Inherits=breeze-dark,gnome,hicolor, so the Breeze
    # set has to be reachable or GTK applications fall back to no icon at all
    # for everything Daemon does not draw itself.
    kdePackages.breeze-icons
  ];

  # Daemon supplies VS Code's complete application/workbench palette; its
  # syntax rules are replaced with Dracula's during the build above. Only the
  # resulting combined theme is installed. ../home/vscode.nix selects it.
  programs.vscode.profiles.default.extensions = [ daemonVscodeTheme ];

  # Kvantum is the application style used by Daemon. The theme directory and
  # its selection file are both managed so System Settings cannot leave an old
  # Kvantum theme active.
  xdg.configFile = (builtins.mapAttrs (_name: link) configLinks) // {
    "Kvantum/daemon-2.0" = {
      source = "${daemonPatched}/Kvantum/daemon-2.0";
    };
    "Kvantum/kvantum.kvconfig".text = ''
      [General]
      theme=daemon-2.0
    '';
  };

  # Install every KDE-facing component supplied by Daemon KDE MK2. Aurorae
  # and Plasma assets are harmless under niri and are ready if Plasma/KWin is
  # started later; Qt/KDE applications use the colours, icons and Kvantum now.
  xdg.dataFile = (builtins.mapAttrs (_name: link) dataLinks) // {
    "aurorae/themes/daemon-2.0" = {
      source = "${daemonTheme}/Window Decorations/daemon-2.0";
    };
    "color-schemes/Daemon2.colors".source = "${daemonPatched}/color-schemes/Daemon2.colors";
    "icons/Daemon-Icons" = {
      source = "${daemonPatched}/icons/Daemon-Icons";
    };
    "konsole/Daemon-2.0.colorscheme".source = "${daemonTheme}/Konsole/Daemon-2.0.colorscheme";
    "plasma/desktoptheme/Daemon-2.0" = {
      source = "${daemonTheme}/Plasma Style/Daemon-2.0";
    };
    "plasma/look-and-feel/Daemon-2.0" = {
      source = "${daemonTheme}/Global Theme/Daemon-2.0";
    };
  };

  # Apply only appearance keys instead of replacing the complete KDE config;
  # Dolphin preferences and other unrelated KDE settings remain untouched.
  home.activation.applyDaemonKdeTheme = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${pkgs.kdePackages.plasma-workspace}/bin/plasma-apply-colorscheme Daemon2

    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group KDE --key widgetStyle kvantum
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group Icons --key Theme Daemon-Icons
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group KDE --key LookAndFeelPackage Daemon-2.0

    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/plasmarc" \
      --group Theme --key name Daemon-2.0
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key library org.kde.kwin.aurorae
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key theme __aurorae__svg__daemon-2.0
  '';
}
