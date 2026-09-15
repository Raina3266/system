# Every theme this configuration applies, in one module. Repository stylesheets
# link in with mkOutOfStoreSymlink; Daemon's KDE, GTK and VS Code themes are
# patched from one pinned checkout so every toolkit shares a palette.
{
  config,
  pkgs,
  repoRoot,
  ...
}:
let
  # --- Repository stylesheets, linked out of the store -----------------------

  link = path: { source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}"; };

  # <path under $XDG_CONFIG_HOME> = <path in this repository>
  configLinks = {
    "preview-panel/preview-panel.css" = "themes/preview-panel.css";
    "media-panel/style.css" = "scripts/media-panel/src/style.css";
    "rofi/rofi-clipboard.rasi" = "themes/rofi-clipboard.rasi";
    "rofi/rofi-finder.rasi" = "themes/rofi-finder.rasi";
    "waybar/style.css" = "themes/waybar/waybar.css";
    # Wayle's style entry point is hardcoded at styles/index.scss and it
    # watches that whole directory. themes/wayle/index.scss imports the
    # per-panel partials next to it; edit those, not the entry file.
    "wayle/styles" = "themes/wayle";
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
  daemonDimPink = "#5A254D";
  daemonYellow = "#FCEE0A";
  daemonTableSeparator = "#6B6670";
  daemonText = "#5DF4FE";
  daemonAlternateBackground = "#210E15";
  daemonBackground = "#180A10";
  daemonSecondaryBackground = "#0F0C17";
  daemonChromeBackground = "#170A0F";

  # One palette across KDE, GTK and VS Code: red for destructive controls and
  # button frames, yellow for separators and popup frames, burgundy surfaces.
  daemonPatched =
    pkgs.runCommandLocal "daemon-2.0-patched"
      {
        nativeBuildInputs = [
          (pkgs.python3.withPackages (pythonPackages: [ pythonPackages.pillow ]))
        ];
      }
      ''
        python3 ${./patch-daemon.py} desktop \
          --source ${daemonTheme} \
          --out "$out" \
          --icon-colour "${daemonRed}" \
          --pink "${daemonPink}" \
          --dim-pink "${daemonDimPink}" \
          --structure-colour "${daemonYellow}" \
          --table-separator-colour "${daemonTableSeparator}" \
          --alternate-background "${daemonAlternateBackground}" \
          --main-background "${daemonBackground}" \
          --secondary-background "${daemonSecondaryBackground}" \
          --chrome-background "${daemonChromeBackground}"

        python3 ${./patch-daemon.py} gtk \
          --source "${daemonTheme}/GTK Theme/Breeze-Dark" \
          --out "$out/share/themes/Daemon-2.0" \
          --icon-colour "${daemonRed}" \
          --pink "${daemonPink}" \
          --dim-pink "${daemonDimPink}" \
          --structure-colour "${daemonYellow}" \
          --table-separator-colour "${daemonTableSeparator}" \
          --alternate-background "${daemonAlternateBackground}" \
          --main-background "${daemonBackground}" \
          --secondary-background "${daemonSecondaryBackground}" \
          --chrome-background "${daemonChromeBackground}"

        # Daemon-Icons inherits "gnome", now only a Nixpkgs alias; retarget it
        # at Adwaita so the chain resolves. The icons came from a read-only
        # store path, so make the directory writable before sed -i.
        chmod +w "$out/icons/Daemon-Icons" "$out/icons/Daemon-Icons/index.theme"
        sed -i 's/Inherits=breeze-dark,gnome,hicolor/Inherits=breeze-dark,Adwaita,hicolor/' \
          "$out/icons/Daemon-Icons/index.theme"
      '';

  # Upstream ships a plain extension directory, so repackage it into the
  # share/vscode/extensions/<unique id> layout Home Manager expects. Its
  # package.json has no publisher, which would name it "undefined_publisher.*".
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
        # Read instead of listing the built directory, which would be an
        # import-from-derivation. All three go into extensions.json, so
        # omitting any is an evaluation error.
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
          --structure-colour "${daemonYellow}" \
          --main-background "${daemonBackground}" \
          --secondary-background "${daemonSecondaryBackground}" \
          --chrome-background "${daemonChromeBackground}"
      '';

  kwriteconfig = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6";
  kdeConfigHome = config.xdg.configHome;

  # Both Kvantum packages ship Kv* colour schemes next to the style plugin.
  # Drop them so System Settings -> Colours offers Daemon2 alone; the plugin
  # itself lives under lib/ and is untouched.
  withoutColorSchemes =
    package:
    package.overrideAttrs (old: {
      postInstall = (old.postInstall or "") + ''
        rm -rf "$out/share/color-schemes"
      '';
    });

  # Libadwaita owns its widget geometry, and Daemon's older Breeze GTK 4 sheet
  # would override it at user priority and break modern dialogs. Import only
  # the generated state/structure patch and semantic colours.
  daemonGtk4UserCss = ''
    @import url("${daemonPatched}/share/themes/Daemon-2.0/gtk-4.0/daemon-overrides.css");

    @define-color accent_color ${daemonPink};
    @define-color accent_bg_color ${daemonDimPink};
    @define-color accent_fg_color ${daemonText};
    @define-color destructive_color ${daemonRed};
    @define-color destructive_bg_color ${daemonRed};
    @define-color destructive_fg_color ${daemonText};
    @define-color window_bg_color ${daemonBackground};
    @define-color window_fg_color ${daemonText};
    @define-color view_bg_color ${daemonSecondaryBackground};
    @define-color view_fg_color ${daemonText};
    @define-color headerbar_bg_color ${daemonChromeBackground};
    @define-color headerbar_fg_color ${daemonText};
    @define-color headerbar_backdrop_color ${daemonBackground};
    @define-color sidebar_bg_color ${daemonAlternateBackground};
    @define-color sidebar_fg_color ${daemonText};
    @define-color card_bg_color ${daemonAlternateBackground};
    @define-color card_fg_color ${daemonText};
    @define-color dialog_bg_color ${daemonBackground};
    @define-color dialog_fg_color ${daemonText};
    @define-color popover_bg_color ${daemonChromeBackground};
    @define-color popover_fg_color ${daemonText};

    :root {
      --accent-color: ${daemonPink};
      --accent-bg-color: ${daemonDimPink};
      --accent-fg-color: ${daemonText};
      --destructive-color: ${daemonRed};
      --destructive-bg-color: ${daemonRed};
      --destructive-fg-color: ${daemonText};
      --window-bg-color: ${daemonBackground};
      --window-fg-color: ${daemonText};
      --view-bg-color: ${daemonSecondaryBackground};
      --view-fg-color: ${daemonText};
      --headerbar-bg-color: ${daemonChromeBackground};
      --headerbar-fg-color: ${daemonText};
      --headerbar-backdrop-color: ${daemonBackground};
      --sidebar-bg-color: ${daemonAlternateBackground};
      --sidebar-fg-color: ${daemonText};
      --card-bg-color: ${daemonAlternateBackground};
      --card-fg-color: ${daemonText};
      --dialog-bg-color: ${daemonBackground};
      --dialog-fg-color: ${daemonText};
      --popover-bg-color: ${daemonChromeBackground};
      --popover-fg-color: ${daemonText};
    }
  '';
in
{
  # Install the complete upstream GTK 2/3/4 theme and select the locally
  # recoloured Daemon variant. Libadwaita applications receive only the shared
  # palette/state override below so their current widget geometry remains intact.
  gtk.theme = {
    name = "Daemon-2.0";
    package = daemonPatched;
  };

  home.packages = with pkgs; [
    (withoutColorSchemes libsForQt5.qtstyleplugin-kvantum)
    (withoutColorSchemes qt6Packages.qtstyleplugin-kvantum)

    # Daemon-Icons declares Inherits=breeze-dark,Adwaita,hicolor (patched in
    # daemonPatched above), so both fallbacks must be reachable or GTK
    # applications fall back to no icon at all for what Daemon does not draw.
    kdePackages.breeze-icons
    adwaita-icon-theme
  ];

  # Daemon supplies VS Code's complete application/workbench palette; its
  # syntax rules are replaced with Dracula's during the build above. Only the
  # resulting combined theme is installed. ../home/vscode.nix selects it.
  programs.vscode.profiles.default.extensions = [ daemonVscodeTheme ];


  # Kvantum is the application style used by Daemon. The theme directory and
  # its selection file are both managed so System Settings cannot leave an old
  # Kvantum theme active.
  xdg.configFile = (builtins.mapAttrs (_name: link) configLinks) // {
    "gtk-3.0/gtk.css".text = ''
      @import url("${daemonPatched}/share/themes/Daemon-2.0/gtk-3.0/gtk.css");
    '';
    "gtk-4.0/gtk.css".text = daemonGtk4UserCss;
    "Kvantum/daemon-2.0" = {
      source = "${daemonPatched}/Kvantum/daemon-2.0";
    };
    "Kvantum/kvantum.kvconfig".text = ''
      [General]
      theme=daemon-2.0
    '';
  };

  # Every KDE-facing Daemon component except Konsole. Aurorae and Plasma assets
  # are inert under niri but ready if it starts later; Qt/KDE apps use the
  # colours, icons and Kvantum now.
  xdg.dataFile = (builtins.mapAttrs (_name: link) dataLinks) // {
    "aurorae/themes/daemon-2.0" = {
      source = "${daemonTheme}/Window Decorations/daemon-2.0";
    };
    "color-schemes/Daemon2.colors".source = "${daemonPatched}/color-schemes/Daemon2.colors";
    "icons/Daemon-Icons" = {
      source = "${daemonPatched}/icons/Daemon-Icons";
    };
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
    # plasma-apply-colorscheme writes into kdeglobals, which apps actually
    # read. Only the file behind the name changes, so drop the recorded name
    # first or the tool skips an already-selected scheme.
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group General --key ColorScheme --delete
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
