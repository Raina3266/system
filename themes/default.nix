# Shared palette and Home Manager theme module. Panels export these colours as
# JSON; GTK, Qt/KDE and VS Code use them when building the Daemon theme.
let
  palette = {
    red = "#D52C35";
    pink = "#D656C7";
    purple = "#BD93F9";
    dimPink = "#5A254D";
    yellow = "#FCEE0A";
    separator = "#6B6670";
    cyan = "#5DF4FE";
    alternateBackground = "#210E15";
    background = "#180A10";
    secondaryBackground = "#0F0C17";
    chromeBackground = "#170A0F";

    # Additional panel shades: an elevated surface, near-white reading text,
    # and a success green used by the desktop panels.
    elevated = "#0E0616";
    foreground = "#F8F8F2";
    green = "#50FA7B";
  };
in
{
  inherit palette;

  module =
    {
      config,
      pkgs,
      repoRoot,
      ...
    }:
    let
      # --- Daemon KDE MK2, from upstream ----------------------------------------

      # Pin the upstream theme so a rebuild cannot silently change its appearance.
      daemonTheme = fetchGit {
        url = "https://github.com/MathisP75/daemon-kde-mk2.git";
        rev = "01bf4df4666e9021ac8013bc2c4eaabc8d312d68";
      };

      paletteArgs = pkgs.lib.cli.toCommandLineShellGNU { } {
        icon-colour = palette.red;
        pink = palette.pink;
        dim-pink = palette.dimPink;
        structure-colour = palette.yellow;
        main-background = palette.background;
        secondary-background = palette.secondaryBackground;
        chrome-background = palette.chromeBackground;
      };

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
              ${paletteArgs} \
              --table-separator-colour "${palette.separator}" \
              --alternate-background "${palette.alternateBackground}"

            python3 ${./patch-daemon.py} gtk \
              --source "${daemonTheme}/GTK Theme/Breeze-Dark" \
              --out "$out/share/themes/Daemon-2.0" \
              ${paletteArgs} \
              --table-separator-colour "${palette.separator}" \
              --alternate-background "${palette.alternateBackground}"

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
              ${paletteArgs}
          '';

      kwriteconfig = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6";
      kdeConfigHome = config.xdg.configHome;

      # Libadwaita owns its widget geometry, and Daemon's older Breeze GTK 4 sheet
      # would override it at user priority and break modern dialogs. Import only
      # the generated state/structure patch and semantic colours.
      daemonGtk4UserCss = ''
        @import url("${daemonPatched}/share/themes/Daemon-2.0/gtk-4.0/daemon-overrides.css");

        @define-color accent_color ${palette.pink};
        @define-color accent_bg_color ${palette.dimPink};
        @define-color accent_fg_color ${palette.cyan};
        @define-color destructive_color ${palette.red};
        @define-color destructive_bg_color ${palette.red};
        @define-color destructive_fg_color ${palette.cyan};
        @define-color window_bg_color ${palette.background};
        @define-color window_fg_color ${palette.cyan};
        @define-color view_bg_color ${palette.secondaryBackground};
        @define-color view_fg_color ${palette.cyan};
        @define-color headerbar_bg_color ${palette.chromeBackground};
        @define-color headerbar_fg_color ${palette.cyan};
        @define-color headerbar_backdrop_color ${palette.background};
        @define-color sidebar_bg_color ${palette.alternateBackground};
        @define-color sidebar_fg_color ${palette.cyan};
        @define-color card_bg_color ${palette.alternateBackground};
        @define-color card_fg_color ${palette.cyan};
        @define-color dialog_bg_color ${palette.background};
        @define-color dialog_fg_color ${palette.cyan};
        @define-color popover_bg_color ${palette.chromeBackground};
        @define-color popover_fg_color ${palette.cyan};

        :root {
          --accent-color: ${palette.pink};
          --accent-bg-color: ${palette.dimPink};
          --accent-fg-color: ${palette.cyan};
          --destructive-color: ${palette.red};
          --destructive-bg-color: ${palette.red};
          --destructive-fg-color: ${palette.cyan};
          --window-bg-color: ${palette.background};
          --window-fg-color: ${palette.cyan};
          --view-bg-color: ${palette.secondaryBackground};
          --view-fg-color: ${palette.cyan};
          --headerbar-bg-color: ${palette.chromeBackground};
          --headerbar-fg-color: ${palette.cyan};
          --headerbar-backdrop-color: ${palette.background};
          --sidebar-bg-color: ${palette.alternateBackground};
          --sidebar-fg-color: ${palette.cyan};
          --card-bg-color: ${palette.alternateBackground};
          --card-fg-color: ${palette.cyan};
          --dialog-bg-color: ${palette.background};
          --dialog-fg-color: ${palette.cyan};
          --popover-bg-color: ${palette.chromeBackground};
          --popover-fg-color: ${palette.cyan};
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
        libsForQt5.qtstyleplugin-kvantum
        qt6Packages.qtstyleplugin-kvantum

        # Daemon-Icons declares Inherits=breeze-dark,Adwaita,hicolor (patched in
        # daemonPatched above), so both fallbacks must be reachable or GTK
        # applications fall back to no icon at all for what Daemon does not draw.
        kdePackages.breeze-icons
        adwaita-icon-theme
      ];

      # Daemon supplies VS Code's complete application/workbench palette; its
      # syntax rules are replaced with Dracula's during the build above. Only the
      # resulting combined theme is installed; workbench.colorTheme in the live
      # settings.json selects it.
      programs.vscode.profiles.default.extensions = [ daemonVscodeTheme ];

      # Kvantum is the application style used by Daemon. The theme directory and
      # its selection file are both managed so System Settings cannot leave an old
      # Kvantum theme active.
      xdg.configFile = {
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

      # Qt/KDE applications use the colour scheme and icons without a Plasma session.
      xdg.dataFile = {
        "fcitx5/themes/cyberpunk/theme.conf".source =
          config.lib.file.mkOutOfStoreSymlink "${repoRoot}/themes/fcitx5.conf";
        "color-schemes/Daemon2.colors".source = "${daemonPatched}/color-schemes/Daemon2.colors";
        "icons/Daemon-Icons" = {
          source = "${daemonPatched}/icons/Daemon-Icons";
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

      '';
    };
}
