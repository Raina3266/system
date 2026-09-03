# SwayNC: the notification daemon and the control center behind the bar's
# centre button.
#
# The control center is the single popup that owns the volume slider, the media
# overview, today's calendar and tasks, and the system readings; Waybar keeps
# only the button that opens it. Imported by ../../nixos/default.nix rather than
# by ../default.nix because the package override below is a Nixpkgs overlay, and
# Home Manager here uses global pkgs.
{ ... }:
{
  nixpkgs.overlays = [
    (final: prev: {
      swaynotificationcenter = prev.swaynotificationcenter.overrideAttrs (oldAttrs: {
        # Two additions, both driven by a command so that everything which
        # knows about players, calendars or sensors stays in this repository's
        # own programs rather than moving into Vala:
        #
        #   label-exec    gives the stock `label` widget an
        #                 `exec`/`interval`/`pango-markup` triple. Without
        #                 `exec` it behaves exactly as it does upstream.
        #   media-widget  adds a `media` widget: a list of rows built from a
        #                 command's output, each with a play/pause button, a
        #                 progress bar and a volume slider that call back into
        #                 that command.
        #
        # Both are checked against v0.12.5, v0.12.6 and upstream main.
        patches = (oldAttrs.patches or [ ]) ++ [
          ./label-exec.patch
          ./media-widget.patch
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

        # SwayNC hands a toggle button's new state to its command in the
        # environment, so set that state rather than toggling blind and hoping
        # the button and the daemon still agree.
        dnd = pkgs.writeShellScript "swaync-dnd" ''
          if [ "''${SWAYNC_TOGGLE_STATE-}" = "true" ]; then
            exec ${swaync} --dnd-on --skip-wait
          fi
          exec ${swaync} --dnd-off --skip-wait
        '';

        # waybar-ycal's popup keeps today's Google Calendar events and Tasks in
        # a cache file, so the panel row reads that rather than starting Python
        # and a set of API calls of its own. Events are plain strings; tasks are
        # objects carrying a done flag, and the open ones come first because
        # they are the part that still needs doing.
        ycalRows = pkgs.writeText "swaync-ycal.jq" ''
          def paint($colour; $text):
            "<span foreground=\"" + $colour + "\">" + ($text | @html) + "</span>";
          def cell($icon; $colour; $text):
            paint("#ff7edb"; $icon) + "  " + paint($colour; $text);

          [ (.[$today] // [])[]
            | if type == "object"
              then { order: (if .done then 2 else 0 end),
                     icon: (if .done then "󰄲" else "󰄱" end),
                     colour: (if .done then "#5c6776" else "#cbe3e7" end),
                     title: .title }
              else { order: 1, icon: "󰃭", colour: "#cbe3e7", title: . }
              end
          ]
          | sort_by(.order)
          | if length == 0 then
              cell("󰃭"; "#5c6776"; "Nothing scheduled today")
            else
              ( (.[:$limit] | map(cell(.icon; .colour; .title)))
                + (if length > $limit
                   then [ paint("#5c6776"; "+ " + (length - $limit | tostring) + " more") ]
                   else [] end)
              ) | join("\n")
            end
        '';

        # A day the cache has nothing for renders the same as a day it has not
        # heard about yet, so an absent cache is simply an empty one.
        noEvents = pkgs.writeText "swaync-ycal-empty.json" "{}";

        ycal = pkgs.writeShellScript "swaync-ycal" ''
          cache="''${SWAYNC_YCAL_CACHE:-$HOME/.cache/waybar-ycal/events.json}"
          [ -r "$cache" ] || cache=${noEvents}
          ${pkgs.jq}/bin/jq -r \
            --arg today "$(${pkgs.coreutils}/bin/date +%F)" \
            --argjson limit 3 \
            -f ${ycalRows} \
            "$cache" 2>/dev/null \
            || printf '%s\n' '<span foreground="#5c6776">󰃭  Calendar unavailable</span>'
        '';
      in
      {
        home.packages = [ repoPackages.swayncSysmon ];

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
            control-center-height = 680;
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

            # A header of pill buttons, the system volume, the media list,
            # then the two `label` rows that render a program's output, then the
            # notifications. Brightness and the session's power actions are
            # deliberately not here: brightness stays on the function keys, and
            # a power menu on a panel that opens under the pointer is a
            # mis-click waiting to happen.
            widgets = [
              "menubar#header"
              "volume"
              "media"
              "label#ycal"
              "label#sysmon"
              "notifications"
            ];

            widget-config = {
              # Quick toggles, the way a quick-settings shade draws them.
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
              };

              # The system's own output level. Per-application volume is not
              # listed here any more: the media rows below carry a slider each,
              # which is the same control against the thing you were looking
              # for when you opened the panel.
              volume = {
                label = "󰕾";
                show-per-app = false;
              };

              # Every player that is playing or paused, each with a play/pause
              # button, a progress bar and its own volume. media-control renders
              # the rows and receives the button and slider back, so "the
              # current player" means one thing here, on the bar button, and in
              # the Rofi menu.
              media = {
                exec = "${mediaControl} players";
                toggle-command = "${mediaControl} play-pause \"$id\"";
                volume-command = "${mediaControl} volume \"$id\" \"$value\"";
                interval = 1;
                empty-text = "Nothing is playing";
              };

              # Today's events and tasks, from waybar-ycal's own cache. Clicking
              # the bar's calendar module still opens its full popup.
              "label#ycal" = {
                text = "Reading calendar…";
                max-lines = 4;
                exec = "${ycal}";
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
