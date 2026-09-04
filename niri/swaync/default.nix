# SwayNC: the notification daemon and the control center behind the bar's
# centre button.
#
# The control center is the single popup that owns the media list, today's
# calendar and tasks, and the system readings; Waybar keeps only the button that
# opens it. Every row it shows is rendered by one of this repository's own
# programs — `media-control` and `swaync-panel` — so the only thing here is
# configuration. Imported by ../../nixos/default.nix rather than
# by ../default.nix because the package override below is a Nixpkgs overlay, and
# Home Manager here uses global pkgs.
{ ... }:
{
  # Keep Chrome on org.freedesktop.Notifications. Its web notifications carry
  # the transient hint, so the retention patch and matching rule below also
  # make them remain in SwayNC's control-center history after the popup closes.
  environment.etc."opt/chrome/policies/managed/swaync-notifications.json".text =
    builtins.toJSON { AllowSystemNotifications = true; };

  nixpkgs.overlays = [
    (final: prev: {
      swaynotificationcenter = prev.swaynotificationcenter.overrideAttrs (oldAttrs: {
        # Three small additions. The command-driven widgets keep everything
        # which knows about players, calendars or sensors in this repository's
        # own programs rather than moving it into Vala:
        #
        #   label-exec    gives the stock `label` widget an
        #                 `exec`/`interval`/`pango-markup` triple. Without
        #                 `exec` it behaves exactly as it does upstream.
        #   media-widget  adds a `media` widget: a list of rows built from a
        #                 command's output, each with a play/pause button, a
        #                 progress bar and a volume slider that call back into
        #                 that command.
        #   retain-       lets an explicit `notification-visibility` rule with
        #   transient     state `enabled` retain a sender's transient popups in
        #                 the control center as well as showing them.
        #
        # All three are checked against v0.12.5, v0.12.6 and upstream main.
        patches = (oldAttrs.patches or [ ]) ++ [
          ./label-exec.patch
          ./media-widget.patch
          ./retain-transient.patch
        ];
      });
    })
  ];

  home-manager.sharedModules = [
    (
      {
        lib,
        pkgs,
        repoPackages,
        ...
      }:
      let
        swaync = "${pkgs.swaynotificationcenter}/bin/swaync-client";
        mediaControl = "${repoPackages.mediaControl}/bin/media-control";
        panel = "${repoPackages.swayncPanel}/bin/swaync-panel";
      in
      {
        home.packages = [ repoPackages.swayncPanel ];

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

            # Sized like a phone's quick-settings shade rather than a sidebar:
            # narrow enough to read as a popup, and only as tall as the widgets
            # plus a few notifications need.
            control-center-layer = "top";
            control-center-positionX = "right";
            control-center-positionY = "top";
            control-center-width = 380;
            control-center-height = 640;
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

            # Chrome marks web notifications transient. Match its desktop ID
            # and explicitly enable them; retain-transient.patch interprets
            # that explicit rule as "popup and keep in the control center".
            notification-visibility.retain-chrome = {
              state = "enabled";
              desktop-entry = "^(google-chrome|chromium|chromium-browser|chrome-.*)$";
            };

            timeout = 8;
            timeout-low = 4;
            timeout-critical = 0;
            transition-time = 200;
            hide-on-clear = false;
            hide-on-action = true;
            keyboard-shortcuts = true;

            # A header of pill buttons, the calendar, notifications, the media
            # list, then the system readings. Three
            # things are deliberately absent: a system volume slider, because
            # every media row carries the one that matters; brightness, which
            # stays on the function keys; and the session's power actions,
            # because a panel that opens under the pointer is a mis-click
            # waiting to happen.
            widgets = [
              "menubar#header"
              "label#calendar"
              "notifications"
              "media"
              "label#sysmon"
            ];

            widget-config = {
              notifications.vexpand = false;

              # Quick toggles, the way a quick-settings shade draws them.
              "menubar#header" = {
                "buttons#quick" = {
                  position = "left";
                  actions = [
                    {
                      label = "󰂛  DND";
                      type = "toggle";
                      command = "${panel} dnd";
                      update-command = "${swaync} --get-dnd --skip-wait";
                    }
                    {
                      label = "󰆴  Clear";
                      command = "${swaync} --close-all --skip-wait";
                      type = "normal";
                    }
                  ];
                };
              };

              # Every player that is playing or paused, each with a play/pause
              # button, a progress bar that can be dragged to seek, and its own
              # volume. media-control renders the rows and receives the button
              # and both sliders back, so "the current player" means one thing
              # here, on the bar button, and in the Rofi menu.
              #
              # There is no system volume slider above this any more: the row
              # for the thing you can hear is the one you were reaching for.
              media = {
                exec = "${mediaControl} players";
                toggle-command = "${mediaControl} play-pause \"$id\"";
                volume-command = "${mediaControl} volume \"$id\" \"$value\"";
                seek-command = "${mediaControl} seek \"$id\" \"$value\"";
                interval = 1;
                empty-text = "Nothing is playing";
              };

              # Today's events and tasks, from waybar-ycal's own cache.
              # Clicking the bar's calendar module still opens its full popup.
              "label#calendar" = {
                text = "Reading calendar…";
                max-lines = 4;
                exec = "${panel} calendar";
                interval = 60;
                pango-markup = true;
              };

              # Live CPU, memory, temperature, disk and network readings, two to
              # a line. The `exec` key comes from ./label-exec.patch; swaync
              # also re-runs the command whenever the control center opens, so
              # the figures are never older than the panel.
              "label#sysmon" = {
                text = "Reading sensors…";
                max-lines = 4;
                exec = "${panel} sysmon";
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
