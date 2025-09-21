{pkgs, lib, ...}: 
let 
  isNixOS = true;
in 
{
  programs.tmux = {
    enable = true;
    prefix = "C-a";
    plugins = with pkgs.tmuxPlugins; [
      {
        plugin = gruvbox;
        extraConfig = ''
          run ${gruvbox}/tmux-gruvbox.nix
          set -g @tmux-gruvbox 'dark'
        '';
      }
    ]; 

    extraConfig = ''
      set -g default-terminal "xterm-256color"
      set -ag terminal-overrides ",xterm-256color:RGB:Sxl"

      set -s extended-keys always
      set -as terminal-features 'xterm-kitty*:extkeys'

      set -gq allow-passthrough on

      bind -n M-x split-window -v -c "#{pane_current_path}"
      bind -n M-v split-window -h -c "#{pane_current_path}"
      bind c new-window -c "#{pane_current_path}"

      set-option -g automatic-rename-format '#{b:pane_current_path}'
    '';
  };

}
