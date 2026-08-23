# Rofi-backed utility modules on the right side of the top bar.
{ pkgs, packages }:
{
  homeConfig.home.packages = [
    pkgs.wl-clipboard
    packages.previewPanel
    packages.rofiClipboard
    packages.rofiNetworkManager
    packages.rofiAudio
  ];

  modules = {
    "custom/network" = {
      exec = "${packages.rofiNetworkManager}/bin/rofi-network-manager status";
      interval = 5;
      return-type = "json";
      tooltip = true;
      escape = false;
      on-click = "${packages.rofiNetworkManager}/bin/rofi-network-manager";
    };

    "custom/timer" = {
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.waybarTimer}/bin/waybar-timer";
      format = "{}";
      return-type = "json";
      tooltip = true;
      escape = false;
      "restart-interval" = 1;
      "exec-on-event" = false;
      on-click = "${packages.waybarTimer}/bin/waybar-timer add";
      on-click-middle = "${packages.waybarTimer}/bin/waybar-timer toggle";
      on-click-right = "${packages.waybarTimer}/bin/waybar-timer clear";
    };

    "custom/clipboard" = {
      exec = "${packages.withParentDeath}/bin/with-parent-death ${packages.rofiClipboard}/bin/rofi-clipboard status";
      return-type = "json";
      tooltip = true;
      escape = false;
      "restart-interval" = 1;
      "exec-on-event" = false;
      on-click = "${packages.rofiClipboard}/bin/rofi-clipboard";
      on-click-right = "${packages.rofiClipboard}/bin/rofi-clipboard clear";
    };

    "custom/audio" = {
      exec = "${packages.rofiAudio}/bin/rofi-audio status";
      interval = 5;
      return-type = "json";
      tooltip = true;
      escape = false;
      on-click = "${packages.rofiAudio}/bin/rofi-audio";
      on-click-right = "${packages.rofiAudio}/bin/rofi-audio bluetooth-power toggle";
    };

    tray = {
      icon-size = 18;
      spacing = 10;
    };
  };
}
