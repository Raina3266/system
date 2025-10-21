{
  pkgs,
  lib,
  nixosConfig ? null,
  ...
}:
let
  isNixOS = nixosConfig != null;
in
{
  programs.zed-editor.enable = true;
  programs.zed-editor.package = lib.mkIf (!isNixOS) (
    pkgs.writeShellScriptBin "zeditor" ''
      ${pkgs.nixgl.nixVulkanIntel}/bin/nixVulkanIntel ${pkgs.zed-editor}/bin/zeditor "$@"
    ''
  );

  # xdg.configFile."zed/settings.json".text = ''
  #   {
  #     "agent": {
  #       "dock": "right"
  #     },
  #     "inlay_hints": {
  #       "enabled": true,
  #       "show_value_hints": true,
  #       "show_type_hints": true,
  #       "show_parameter_hints": true,
  #       "show_other_hints": true,
  #       "show_background": false,
  #       "edit_debounce_ms": 700,
  #       "scroll_debounce_ms": 50,
  #       "toggle_on_modifiers_press": {
  #         "control": false,
  #         "alt": false,
  #         "shift": false,
  #         "platform": false,
  #         "function": false
  #       }
  #     },
  #     "vim_mode": true,
  #     "auto_install_extensions": {
  #       "nix": true
  #     },
  #     "icon_theme": {
  #       "mode": "system",
  #       "light": "Material Icon Theme",
  #       "dark": "Material Icon Theme"
  #     },
  #     "ui_font_size": 18,
  #     "buffer_font_size": 18,
  #     "theme": {
  #       "mode": "system",
  #       "light": "Gruvbox Dark Hard",
  #       "dark": "Gruvbox Dark Hard"
  #     },
  #     "hover_popover_enabled": false,
  #     "disable_ai": true,
  #     "autosave": {
  #       "after_delay": {
  #         "milliseconds": 0
  #       }
  #     },
  #     "lsp": {
  #       "rust-analyzer": {
  #         "binary": {
  #           "path": "rust-analyzer"
  #         },
  #         "initialization_options": {
  #           "check": {
  #             "command": "clippy"
  #           }
  #         }
  #       },
  #     }
  #   }

  #   // this IS managed by nix
  # '';
}
