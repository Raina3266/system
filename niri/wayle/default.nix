# Wayle is the notification daemon: it owns org.freedesktop.Notifications,
# draws the popups, and keeps the history. Waybar remains the visible bar and
# Wayle's native dashboard also owns notification history and the seven-day
# agenda. Its own bar is visually hidden. The dashboard, Wi-Fi manager, media
# panel, and Bluetooth/audio panel opened from Waybar use separate layer-shell
# windows, avoiding GTK popup-grab restrictions.
{ ... }:
let
  # Flakes are copied into a source store path whose hash changes whenever an
  # unrelated tracked file changes. Copy each patch to its own content-based
  # path so those edits do not invalidate the expensive Wayle build.
  stableWaylePatch =
    path:
    builtins.path {
      inherit path;
      name = builtins.baseNameOf path;
      recursive = false;
    };
in
{
  environment.etc."opt/chrome/policies/managed/wayle-notifications.json".text =
    builtins.toJSON { AllowSystemNotifications = true; };

  nixpkgs.overlays = [
    (final: prev: {
      # Correct browser replay/backward seeks before mprisence publishes them.
      mprisence = prev.mprisence.overrideAttrs (oldAttrs: {
        patches = (oldAttrs.patches or [ ]) ++ [ ./mprisence-position.patch ];
      });

      wayle = prev.wayle.overrideAttrs (oldAttrs: {
        patches =
          (oldAttrs.patches or [ ])
          ++ builtins.map stableWaylePatch [
            ./notification-history.patch
            ./dashboard-waybar-host.patch
            ./dashboard-layer-window.patch
            ./wayle-wifi.patch
            ./dashboard-power-profile.patch
            ./wayle-media-panel.patch
            ./dashboard-notifications.patch
            ./wayle-audio-panel.patch
            ./dashboard-slim.patch
            ./dashboard-polish.patch
          ];
      });
    })
  ];

  home-manager.sharedModules = [
    (
      { lib, pkgs, ... }:
      {
        services.wayle = {
          enable = true;
          autoInstallDependencies = false;
          settings = {
            general = {
              "font-sans" = "Noto Sans";
              "font-mono" = "JetBrains Mono";
            };

            styling = {
              scale = 0.9;
              rounding = "sm";
              "theme-provider" = "wayle";
              palette = {
                bg = "#180A10";
                surface = "#210E15";
                elevated = "#0E0616";
                fg = "#F8F8F2";
                "fg-muted" = "#6B6670";
                primary = "#D656C7";
                red = "#D52C35";
                yellow = "#FCEE0A";
                green = "#50FA7B";
                blue = "#5DF4FE";
              };
            };

            bar = {
              # `show = false` keeps Wayle's own bar visually hidden. External
              # dashboard, Wi-Fi, media and audio requests use monitor-local
              # layer surfaces.
              location = "top";
              layer = "overlay";
              "dropdown-opacity" = 100;
              # The click originates in Waybar, not Wayle. An autohide GTK
              # popover would request an xdg_popup grab using an input serial
              # Wayle never received, so the compositor dismisses it immediately.
              # The same Waybar button closes its panel on the next click.
              "dropdown-autohide" = false;
              layout = [
                {
                  monitor = "*";
                  show = false;
                  left = [ ];
                  center = [ ];
                  right = [ ];
                }
              ];
            };

            osd.enabled = false;
            wallpaper."engine-enabled" = false;

            modules.notifications = {
              "icon-show" = false;
              "label-show" = false;
              "popup-position" = "top-right";
              "popup-max-visible" = 5;
              "popup-stacking-order" = "newest-first";
              "popup-duration" = 8000;
              "popup-hover-pause" = true;
              "popup-margin-x" = 6.0;
              "popup-margin-y" = 46.0;
              "popup-gap" = 6.0;
              "popup-monitor" = "primary";
              "popup-layer" = "overlay";
              "popup-close-behavior" = "dismiss";
              "popup-shadow" = true;
              "popup-urgency-bar" = "low";
            };
          };
        };

        # GNOME owns org.freedesktop.Notifications in its own session.
        systemd.user.services.wayle = {
          Unit = {
            ConditionEnvironment = lib.mkForce [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            # Stop a service left running by the previous Home Manager
            # generation before Wayle claims org.freedesktop.Notifications.
            Conflicts = [ "swaync.service" ];
          };
          Service = {
            RestartSec = 3;
            # The standalone network panel pipes the Wi-Fi payload over stdin, so
            # saved passwords never appear in argv or a temporary file.
            Environment = "WAYLE_QRENCODE=${lib.getExe' pkgs.qrencode "qrencode"}";
          };
        };
      }
    )
  ];
}
