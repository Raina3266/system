# Zed editor configuration.
#
# All Zed settings are managed here via home-manager's
# programs.zed-editor.userSettings, which writes to
# ~/.config/zed/settings.json. Edit this file to change
# Zed settings — do not edit settings.json directly, as
# home-manager will overwrite it on switch.
{
  pkgs,
  inputs,
  ...
}:
{
  programs.zed-editor = {
    enable = true;
    # Nightly from the upstream flake (matches zed.cachix.org; see flake.nix).
    package = inputs.zed.packages.${pkgs.stdenv.hostPlatform.system}.default;

    userSettings = {
      # ── Theme & fonts ───────────────────────────────────────────────
      theme = {
        mode = "dark";
        light = "Catppuccin Latte";
        dark = "Dracula Solid";
      };
      ui_font_size = 18;
      buffer_font_size = 18;

      # ── Terminal ────────────────────────────────────────────────────
      terminal = {
        shell.program = "fish";
        theme = "Neon";
      };

      # ── Editor behavior ─────────────────────────────────────────────
      autosave = "on_focus_change";
      cli_default_open_behavior = "existing_window";
      edit_predictions.mode = "subtle";

      # ── Panel layout ────────────────────────────────────────────────
      outline_panel.dock = "left";
      git_panel.dock = "left";
      project_panel.dock = "left";

      # ── Agent (AI assistant) ────────────────────────────────────────
      agent = {
        dock = "right";
        default_model = {
          provider = "openrouter";
          model = "z-ai/glm-5.2";
          enable_thinking = true;
        };
        favorite_models = [ ];
        model_parameters = [ ];
        tool_permissions = {
          tools.terminal.default = "allow";
        };
      };
    };
  };
}
