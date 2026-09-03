# SwayNC: the notification daemon and the control center behind the top-right
# bell in Waybar.
#
# The control center is the single popup that owns brightness, volume, the
# system readings and the session's power actions; Waybar keeps only the button
# that opens it. Imported by ../../nixos/default.nix rather than by ../default.nix
# because the package override below is a Nixpkgs overlay, and Home Manager here
# uses global pkgs.
{ ... }:
{
  nixpkgs.overlays = [
    (final: prev: {
      swaynotificationcenter = prev.swaynotificationcenter.overrideAttrs (oldAttrs: {
        # Upstream's `label` widget shows a fixed string. The patch gives it an
        # `exec`/`interval`/`pango-markup` triple so the widget can display a
        # command's output, which is what the system monitor row needs; without
        # `exec` the widget behaves exactly as it does upstream. Checked against
        # v0.12.5, v0.12.6 and upstream main.
        patches = (oldAttrs.patches or [ ]) ++ [ ./label-exec.patch ];
      });
    })
  ];

  home-manager.sharedModules = [
    (
      {
        config,
        lib,
        pkgs,
        repoPackages,
        ...
      }:
      let
        swaync = "${pkgs.swaynotificationcenter}/bin/swaync-client";

        # Reboot, shutdown and logout are unrecoverable from a mis-click on a
        # panel that opens under the pointer, so they confirm through Rofi,
        # which is already the rest of this desktop's dialog toolkit. Suspend
        # does not: it costs a keypress to undo.
        power = pkgs.writeShellScriptBin "swaync-power" ''
          set -euo pipefail

          theme="''${SWAYNC_POWER_THEME:-${config.xdg.configHome}/rofi/rofi-power.rasi}"

          case "''${1-}" in
            suspend)  icon="󰤄"; label="Suspend";   confirm=false; run=(${pkgs.systemd}/bin/systemctl suspend) ;;
            logout)   icon="󰍃"; label="Log out";   confirm=true;  run=(niri msg action quit --skip-confirmation) ;;
            reboot)   icon="󰜉"; label="Restart";   confirm=true;  run=(${pkgs.systemd}/bin/systemctl reboot) ;;
            poweroff) icon="󰐥"; label="Shut down"; confirm=true;  run=(${pkgs.systemd}/bin/systemctl poweroff) ;;
            *)
              echo "usage: swaync-power <suspend|logout|reboot|poweroff>" >&2
              exit 2
              ;;
          esac

          # The control center is a layer-shell overlay and would otherwise sit
          # on top of the confirmation dialog.
          ${swaync} --close-panel --skip-wait || true

          if [ "$confirm" = true ]; then
            choice="$(printf '󰜺  Cancel\n%s  %s\n' "$icon" "$label" \
              | ${pkgs.rofi}/bin/rofi -dmenu -i -no-custom -p "$label?" -theme "$theme")" || exit 0
            case "$choice" in
              *"$label") ;;
              *) exit 0 ;;
            esac
          fi

          exec "''${run[@]}"
        '';

        # SwayNC hands a toggle button's new state to its command in the
        # environment, so set that state rather than toggling blind and hoping
        # the button and the daemon still agree.
        dnd = pkgs.writeShellScript "swaync-dnd" ''
          if [ "''${SWAYNC_TOGGLE_STATE-}" = "true" ]; then
            exec ${swaync} --dnd-on --skip-wait
          fi
          exec ${swaync} --dnd-off --skip-wait
        '';

        action = label: argument: {
          inherit label;
          command = "${power}/bin/swaync-power ${argument}";
          type = "normal";
        };
      in
      {
        home.packages = [
          power
          repoPackages.swayncSysmon
        ];

        services.swaync = {
          enable = true;

          # style.css is not set here: ../../themes/default.nix links
          # themes/swaync.css into place so edits apply without a rebuild
          # (`swaync-client --reload-css` picks them up).
          settings = {
            positionX = "right";
            positionY = "top";
            layer = "overlay";
            layer-shell = true;

            # Sits under the top bar's right edge, below the bell that opens it.
            # Sized like a phone's quick-settings shade rather than a sidebar:
            # narrow enough to read as a popup, and only as tall as the widgets
            # plus a few notifications need.
            control-center-layer = "top";
            control-center-positionX = "right";
            control-center-positionY = "top";
            control-center-width = 380;
            control-center-height = 560;
            control-center-margin-top = 6;
            control-center-margin-right = 6;
            control-center-margin-bottom = 6;
            control-center-margin-left = 0;
            control-center-exclusive-zone = false;
            fit-to-screen = false;

            notification-window-width = 360;
            notification-icon-size = 32;
            notification-body-image-height = 80;
            notification-body-image-width = 160;
            notification-grouping = true;
            image-visibility = "when-available";
            relative-timestamps = true;

            timeout = 8;
            timeout-low = 4;
            timeout-critical = 0;
            transition-time = 200;
            hide-on-clear = false;
            hide-on-action = true;
            keyboard-shortcuts = true;

            # Five rows, in the order a quick-settings shade uses them: one
            # header of pill buttons, the two sliders, the readings, the list.
            # There is no separate title or Do Not Disturb row — both collapse
            # into the header, which is most of what makes the panel short.
            widgets = [
              "menubar#header"
              "backlight"
              "volume"
              "label#sysmon"
              "notifications"
            ];

            widget-config = {
              # Quick toggles on the left, the session menu on the right. The
              # menu is a revealer: clicking 󰐥 slides the four power actions
              # open underneath the row, so they cost no height until asked for.
              "menubar#header" = {
                "buttons#quick" = {
                  position = "left";
                  actions = [
                    {
                      label = "󰂛  DND";
                      type = "toggle";
                      command = "${dnd}";
                      update-command = "${swaync} --get-dnd --skip-wait";
                    }
                    {
                      label = "󰆴  Clear";
                      command = "${swaync} --close-all --skip-wait";
                      type = "normal";
                    }
                  ];
                };

                "menu#power" = {
                  label = "󰐥";
                  position = "right";
                  animation-type = "slide_down";
                  animation-duration = 200;
                  actions = [
                    (action "󰤄   Suspend" "suspend")
                    (action "󰍃   Log out" "logout")
                    (action "󰜉   Restart" "reboot")
                    (action "󰐥   Shut down" "poweroff")
                  ];
                };
              };

              backlight = {
                label = "󰃠";
                # The panel that ../config.kdl's brightness keys drive. A
                # different GPU reports a different name under
                # /sys/class/backlight (amdgpu_bl0, acpi_video0, …).
                device = "intel_backlight";
                subsystem = "backlight";
                min = 5;
              };

              volume = {
                label = "󰕾";
                show-per-app = true;
                show-per-app-icon = true;
                show-per-app-label = true;
                expand-per-app = false;
                empty-list-label = "Nothing is playing";
                expand-button-label = "󰅀";
                collapse-button-label = "󰅃";
              };

              # Live CPU, memory, temperature, disk and network readings. The
              # `exec` key comes from ./label-exec.patch; swaync also re-runs
              # the command whenever the control center opens, so the figures
              # are never older than the panel.
              "label#sysmon" = {
                text = "Reading sensors…";
                max-lines = 6;
                exec = "${repoPackages.swayncSysmon}/bin/swaync-sysmon";
                interval = 3;
                pango-markup = true;
              };
            };
          };
        };

        # GNOME runs its own notification daemon, and two owners of
        # org.freedesktop.Notifications cannot coexist, so keep this one to the
        # Niri session — the same condition ../waybar/default.nix uses.
        systemd.user.services.swaync = {
          Unit.ConditionEnvironment = lib.mkForce [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          Service.RestartSec = 3;
        };
      }
    )
  ];
}
