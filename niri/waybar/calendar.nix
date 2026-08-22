# Google Calendar/Tasks Waybar module and popup service.
{ lib, packages }:
{
  homeConfig = {
    home.packages = [ packages.ycal.package ];

    systemd.user.services.waybar-ycal = {
      Unit = {
        Description = "waybar-ycal: Google Calendar and Tasks popup";
        ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${packages.ycal.package}/bin/waybar-ycal-popup";
        Restart = lib.mkForce "on-failure";
        RestartSec = 3;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };

  modules."custom/ycal" = {
    return-type = "json";
    interval = 60;
    exec = packages.ycal.barExec;
    on-click = packages.ycal.toggle;
  };
}
