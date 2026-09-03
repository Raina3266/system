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
  
  # ── btop ──────────────────────────────────────────────────────────────────
  # Stands in for the sensor pages System Monitor had: qps covers processes,
  # this covers the graphs. ../../themes/default.nix supplies the palette.
  programs.btop = {
    enable = true;

    settings = {
      color_theme = "Daemon-2.0";
      # Ghostty already paints the window; a second, slightly different black
      # behind every box is worse than none.
      theme_background = false;
      truecolor = true;
      rounded_corners = true;
      graph_symbol = "braille";

      vim_keys = true;
      update_ms = 1000;
      # Keep collecting while the terminal is hidden, so a graph read after
      # switching back covers the time away rather than starting empty.
      background_update = true;

      shown_boxes = "cpu mem net proc";
      mem_below_net = true;
      presets = "net:0:default cpu:0:default,proc:0:default";

      # CPU: frequency and per-core temperatures, with the lower graph on
      # iowait so a stalled disk is visible next to load rather than hidden
      # in it.
      show_cpu_freq = true;
      cpu_graph_upper = "total";
      cpu_graph_lower = "iowait";
      cpu_invert_lower = true;
      check_temp = true;
      show_coretemp = true;
      temp_scale = "celsius";

      # Memory: meters and graphs for every category, swap in the same box,
      # and disks with their read/write rates.
      mem_graphs = true;
      show_swap = true;
      swap_disk = true;
      show_disks = true;
      only_physical = true;
      show_io_stat = true;
      io_mode = true;
      io_graph_combined = false;

      net_auto = true;
      net_sync = true;
      net_iface = "";

      # Processes: tree, a CPU history graph per process, memory in bytes
      # rather than percent, and kernel threads left in the list.
      proc_tree = true;
      proc_sorting = "cpu lazy";
      proc_cpu_graphs = true;
      proc_mem_bytes = true;
      proc_gradient = true;
      proc_filter_kernel = false;

      show_uptime = true;
      show_battery = true;
      show_battery_watts = true;
      clock_format = "%X";
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
