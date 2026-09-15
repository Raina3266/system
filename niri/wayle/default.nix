# Wayle is the notification daemon: it owns org.freedesktop.Notifications and
# the history, and its dashboard owns the agenda. Waybar stays the visible bar,
# so Wayle's own is hidden. Dropdowns opened from Waybar use monitor-local
# layer-shell windows, avoiding GTK popup-grab limits. Audio is Wayle's own;
# audio-control adds only the profile handling it lacks.
{ repoPackages, ... }:
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
        # One delta against pristine Wayle replaces the old 17-patch sequence,
        # avoiding dependent hunks. Presentation lives in themes/wayle/ and
        # hot-reloads.
        patches = (oldAttrs.patches or [ ]) ++ [ (stableWaylePatch ./wayle-features.patch) ];
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
              # dropdown requests use monitor-local layer surfaces with their
              # own click-away backdrop.
              location = "top";
              layer = "overlay";
              "dropdown-opacity" = 100;
              # D-Bus panels no longer use GTK popovers, so ordinary Wayle
              # popovers can safely use their native outside-click dismissal.
              "dropdown-autohide" = true;
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
            # Wayle renders both panels. External Rust helpers supply only the
            # behaviour that is not already implemented by Wayle itself.
            Environment = [
              "WAYLE_NETWORK_MANAGER=${repoPackages.networkManager}/bin/network-manager"
              "WAYLE_AUDIO_HELPER=${repoPackages.audioControl}/bin/audio-control"
            ];
          };
        };
      }
    )
  ];
}
