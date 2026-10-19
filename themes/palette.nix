# The one palette every toolkit shares. themes/default.nix patches Daemon's
# KDE, GTK and VS Code themes from these values; niri/quickshell/default.nix
# renders the media panel's Palette.qml from the same attrset, so a colour
# change here cannot drift between toolkits.
{
  red = "#D52C35";
  pink = "#D656C7";
  dimPink = "#5A254D";
  yellow = "#FCEE0A";
  separator = "#6B6670";
  cyan = "#5DF4FE";
  alternateBackground = "#210E15";
  background = "#180A10";
  secondaryBackground = "#0F0C17";
  chromeBackground = "#170A0F";

  # Wayle's styling.palette shades that have no Daemon counterpart: the panel
  # surface one step above the background, the near-white reading text, and
  # the success green. The Quickshell media panel reuses the same mapping.
  elevated = "#0E0616";
  foreground = "#F8F8F2";
  green = "#50FA7B";
}
