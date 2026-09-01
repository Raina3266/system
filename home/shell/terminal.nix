{
  pkgs,
  ...
}:
{
  # ── Terminal emulators ───────────────────────────────────────────────────
  programs.ghostty = {
    enable = true;
    enableFishIntegration = true;
    settings = {
      command = "fish";
      theme = "Bright Lights";
      adjust-cell-height = "20%";
      background = "#180A10";
    };
  };

  # ── Bottom ────────────────────────────────────────────────────────────────
  programs.bottom = {
    enable = true;
    settings = {
      # The pinned Bottom version uses styles instead of the old colors table.
      styles =
        let
          # Match the Daemon cyberpunk palette in themes/default.nix.
          background = "#180A10";
          cyan = "#5DF4FE";
          red = "#D52C35";
          pink = "#D656C7";
          dimPink = "#5A254D";
          yellow = "#FCEE0A";
          grey = "#6B6670";
          muted = "#7A9B9F";
          green = "#28C775";
          graphColors = [
            cyan
            pink
            yellow
            red
            green
            muted
          ];
        in
        {
          cpu = {
            all_entry_color = cyan;
            avg_entry_color = yellow;
            cpu_core_colors = graphColors;
          };
          temp_graph.temp_graph_color_styles = graphColors;
          memory = {
            ram_color = pink;
            cache_color = red;
            swap_color = yellow;
            arc_color = cyan;
            gpu_colors = graphColors;
          };
          network = {
            rx_color = cyan;
            tx_color = pink;
            rx_total_color = green;
            tx_total_color = yellow;
          };
          battery = {
            high_battery_color = green;
            medium_battery_color = yellow;
            low_battery_color = red;
          };
          tables.headers = {
            color = yellow;
            bold = true;
          };
          graphs = {
            graph_color = grey;
            legend_text.color = muted;
          };
          widgets = {
            bg_color = background;
            border_color = red;
            selected_border_color = pink;
            widget_title.color = yellow;
            text.color = cyan;
            selected_text = {
              color = cyan;
              bg_color = dimPink;
              bold = true;
            };
            disabled_text.color = muted;
            thread_text.color = green;
          };
        };
    };
  };

  # ── tmux ──────────────────────────────────────────────────────────────────
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
