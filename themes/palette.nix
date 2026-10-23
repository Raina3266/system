# The one palette every toolkit shares. themes/default.nix patches Daemon's
# KDE, GTK and VS Code themes; niri/panels/{media,audio,network,dashboard}.nix
# export JSON for the AGS panels' GTK CSS.
# Generated assets stay outside the live-linked presentation directories.
{
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
}
