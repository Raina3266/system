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

    # ── Extensions ──────────────────────────────────────────────────
    extensions = [
      "catppuccin"
      "dracula"
      "html"
      "kdl"
      "nix"
      "lua"
      "toml"
      "crates-lsp"           # Cargo.toml dependency info, version hints, features
      "make"                 # Makefile syntax (often used in Rust projects)
      "dockerfile"           # Dockerfile syntax (containerising Rust apps)
      "docker-compose"       # docker-compose.yml support
      "gitignore-templates"  # quick .gitignore scaffolding
      "git-firefly"          # git gutter/blame enhancements
      "yaml"                 # CI configs, flake.lock, k8s manifests
      "json5"                # relaxed JSON (used by some Rust tooling)
      "markdown-oxide"       # Markdown LSP (README, docs)
      "github-actions"       # GitHub Actions workflow syntax
      "editorconfig"         # .editorconfig support
    ];

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
