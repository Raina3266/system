{
  pkgs,
  ...
}:
let
  # The structural colour ../../themes/default.nix gives menus, popups and
  # scrollbars everywhere else.
  daemonYellow = "#FCEE0A";
  daemonRed = "#D52C35";
  daemonPink = "#D656C7";
  daemonDimPink = "#5A254D";
  daemonText = "#5DF4FE";
  daemonDimText = "#2E6F76";
  daemonMutedText = "#7A9B9F";
  daemonAlternateBackground = "#210E15";
  white = "#FFFFFF";
in
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
  # this covers the graphs.
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
      proc_colors = true;
      proc_gradient = true;
      proc_filter_kernel = false;

      show_uptime = true;
      show_battery = true;
      show_battery_watts = true;
      clock_format = "%X";
    };

    # btop falls back to its own defaults for anything left out.
    #
    # Frames yellow, text cyan, headings pink, menu shortcut letters red.
    #
    # Numbers coloured by value run white -> pink -> red: load, temperature,
    # memory used and processes climb that way, free memory runs it inverted.
    # Cached memory and network throughput are neither good nor bad, so they
    # stay cyan-to-pink and never reach red.
    themes."Daemon-2.0" = ''
      theme[main_fg]="${daemonText}"
      theme[inactive_fg]="${daemonDimText}"
      theme[title]="${daemonPink}"
      theme[hi_fg]="${daemonRed}"
      theme[selected_bg]="${daemonDimPink}"
      theme[selected_fg]="${white}"

      # Blue is the one colour nothing else here uses, so a followed process
      # takes the pink accent; pausing is worth noticing, so that banner is red.
      theme[followed_bg]="${daemonPink}"
      theme[followed_fg]="${white}"
      theme[proc_follow_bg]="${daemonPink}"
      theme[proc_pause_bg]="${daemonRed}"
      theme[proc_banner_bg]="${daemonDimPink}"
      theme[proc_banner_fg]="${white}"
      theme[graph_text]="${daemonMutedText}"
      theme[meter_bg]="${daemonAlternateBackground}"
      theme[proc_misc]="${daemonPink}"

      theme[cpu_box]="${daemonYellow}"
      theme[mem_box]="${daemonYellow}"
      theme[net_box]="${daemonYellow}"
      theme[proc_box]="${daemonYellow}"
      theme[div_line]="${daemonDimText}"

      theme[cpu_start]="${white}"
      theme[cpu_mid]="${daemonPink}"
      theme[cpu_end]="${daemonRed}"

      theme[temp_start]="${white}"
      theme[temp_mid]="${daemonPink}"
      theme[temp_end]="${daemonRed}"

      theme[used_start]="${white}"
      theme[used_mid]="${daemonPink}"
      theme[used_end]="${daemonRed}"

      # A process name, its thread count, its memory and its cpu share all
      # come from this one ramp, and a name has to read red, so the ramp is
      # held there: those three columns are red with it rather than running
      # white to red like every other value.
      theme[process_start]="${daemonRed}"
      theme[process_mid]="${daemonRed}"
      theme[process_end]="${daemonRed}"

      theme[free_start]="${daemonRed}"
      theme[free_mid]="${daemonPink}"
      theme[free_end]="${white}"

      theme[available_start]="${daemonRed}"
      theme[available_mid]="${daemonPink}"
      theme[available_end]="${white}"

      theme[cached_start]="${white}"
      theme[cached_mid]="${daemonPink}"
      theme[cached_end]="${daemonRed}"

      theme[download_start]="${white}"
      theme[download_mid]="${daemonPink}"
      theme[download_end]="${daemonRed}"

      theme[upload_start]="${white}"
      theme[upload_mid]="${daemonPink}"
      theme[upload_end]="${daemonRed}"
    '';
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
