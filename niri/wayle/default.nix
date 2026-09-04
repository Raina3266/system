# Wayle supplies notification popups and history while the existing Waybar
# remains the visible desktop bar. Upstream only opens dropdowns from its own
# bar, so the local patch adds a small D-Bus/CLI bridge. Wayle keeps one bar
# per output as the GTK anchor for that dropdown. The bar stays mapped even
# though nothing on it is visible: a GTK popover can only be presented from a
# surface the compositor has already mapped, and mapping a layer surface costs
# a configure round-trip, so a bar mapped on demand is never ready in time. The
# patch hides it instead by dropping its background, hiding its sections and
# giving it an empty input region, so it is invisible and clicks pass straight
# through to Waybar underneath.
{ ... }:
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
        patches = (oldAttrs.patches or [ ]) ++ [ ./waybar-dropdown.patch ];
      });
    })
  ];

  home-manager.sharedModules = [
    (
      { lib, ... }:
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
                elevated = "#0F0C17";
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
              location = "top";
              layer = "overlay";
              exclusive = false;
              padding = 0.0;
              "padding-ends" = 0.0;
              "module-gap" = 0.0;
              "background-opacity" = 0;
              "button-opacity" = 0;
              "button-bg-opacity" = 0;
              "button-icon-padding" = 0.25;
              "button-label-padding" = 0.25;
              "dropdown-opacity" = 100;
              # An autohide popover asks the compositor for a popup grab, which
              # is only granted against the serial of an input event Wayle
              # itself received. The click comes from Waybar, so there is no
              # such serial and the grab would dismiss the dropdown instantly.
              # Clicking the Waybar button again closes it.
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
          Service.RestartSec = 3;
        };
      }
    )
  ];
}
