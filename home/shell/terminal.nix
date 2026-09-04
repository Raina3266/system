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

    # btop falls back to its own default theme for every key left out here.
    #
    # Frames are yellow, as menus, popups and scrollbars are everywhere else.
    # Text is cyan - which is what colours a pid and its command line, both
    # drawn from main_fg fading to inactive_fg - headings pink, and the
    # shortcut letter in a menu label red.
    #
    # Every number btop colours by value runs the same three stops: white
    # where the reading wants no attention, pink on the way, red at the end
    # that matters. Load, temperature, memory in use and a process climb from
    # white to red; memory free or available, cached memory and network
    # throughput run it the other way, since for those it is the low end that
    # is worth seeing. Memory that is free or available reads the same way
    # inverted, since there running out is the warning. Cached memory and
    # network throughput are neither good nor bad, so they stay on their own
    # hue - cyan down, pink up - and never turn red.
    themes."Daemon-2.0" = ''
      theme[main_fg]="${daemonText}"
      theme[inactive_fg]="${daemonDimText}"
      theme[title]="${daemonPink}"
      theme[hi_fg]="${daemonRed}"
      theme[selected_bg]="${daemonDimPink}"
      theme[selected_fg]="${white}"
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
      # come from this one ramp, so a name is white while the process is idle
      # and reddens as it works.
      theme[process_start]="${white}"
      theme[process_mid]="${daemonPink}"
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
