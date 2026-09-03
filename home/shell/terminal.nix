{
  pkgs,
  ...
}:
let
  # The same values ../../themes/default.nix hands KDE, GTK and VS Code. btop
  # reads one flat list of colours rather than a theme package, so they are
  # repeated here instead of being patched out of an upstream file.
  daemonRed = "#D52C35";
  daemonPink = "#D656C7";
  # Halfway between the pink accent and red, so a process name shifts along
  # one hue as it heats up rather than crossing two.
  daemonPinkRed = "#D5417E";
  daemonDimPink = "#5A254D";
  daemonYellow = "#FCEE0A";
  daemonSeparator = "#6B6670";
  daemonMutedText = "#7A9B9F";
  daemonText = "#5DF4FE";
  daemonAlternateBackground = "#210E15";
  daemonBackground = "#180A10";
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
      # The name takes its colour from the process gradient below, and the pid
      # and command line fade with distance down the list.
      proc_colors = true;
      proc_gradient = true;
      proc_filter_kernel = false;

      show_uptime = true;
      show_battery = true;
      show_battery_watts = true;
      clock_format = "%X";
    };

    # Yellow frames the boxes and nothing else, as it frames menus and popups
    # across the rest of the desktop. Text is cyan, headings pink, shortcut
    # keys red; meters and graphs run cool to hot, cyan through pink to red.
    #
    # btop draws a process name from one colour and its pid and command line
    # from another, so the pid cannot differ from the path: both take the
    # greyscale ramp below, which fades with distance down the list.
    themes."Daemon-2.0" = ''
      theme[main_bg]="${daemonBackground}"
      theme[main_fg]="${daemonText}"
      theme[title]="${daemonPink}"
      theme[hi_fg]="${daemonRed}"
      theme[selected_bg]="${daemonDimPink}"
      theme[selected_fg]="${daemonText}"
      theme[inactive_fg]="${daemonSeparator}"
      theme[graph_text]="${daemonMutedText}"
      theme[meter_bg]="${daemonAlternateBackground}"
      theme[proc_misc]="${daemonPink}"

      theme[cpu_box]="${daemonYellow}"
      theme[mem_box]="${daemonYellow}"
      theme[net_box]="${daemonYellow}"
      theme[proc_box]="${daemonYellow}"
      theme[div_line]="${daemonSeparator}"

      theme[temp_start]="${daemonText}"
      theme[temp_mid]="${daemonPink}"
      theme[temp_end]="${daemonRed}"

      theme[cpu_start]="${daemonText}"
      theme[cpu_mid]="${daemonPink}"
      theme[cpu_end]="${daemonRed}"

      theme[used_start]="${daemonText}"
      theme[used_mid]="${daemonPink}"
      theme[used_end]="${daemonRed}"

      theme[free_start]="${daemonSeparator}"
      theme[free_mid]="${daemonMutedText}"
      theme[free_end]="${daemonText}"

      theme[cached_start]="${daemonDimPink}"
      theme[cached_mid]="${daemonPink}"
      theme[cached_end]="${daemonText}"

      theme[available_start]="${daemonSeparator}"
      theme[available_mid]="${daemonText}"
      theme[available_end]="${daemonPink}"

      theme[download_start]="${daemonMutedText}"
      theme[download_mid]="${daemonText}"
      theme[download_end]="${daemonPink}"

      theme[upload_start]="${daemonDimPink}"
      theme[upload_mid]="${daemonPink}"
      theme[upload_end]="${daemonRed}"

      # A process name is drawn from this ramp and its pid and command line
      # from main_fg fading to inactive_fg. Keeping the name off that cyan to
      # grey axis is what makes the three columns tell apart: names run pink
      # to red by how busy the process is, pids and paths stay cyan to grey.
      theme[process_start]="${daemonPink}"
      theme[process_mid]="${daemonPinkRed}"
      theme[process_end]="${daemonRed}"
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
