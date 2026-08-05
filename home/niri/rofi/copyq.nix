{ pkgs, ... }:
let
  # Custom Rust script to query CopyQ items and select via Rofi
  rofi-copyq = pkgs.writeShellApplication {
    name = "rofi-copyq";
    runtimeInputs = with pkgs; [
      copyq
      rofi
      coreutils
      gnugrep
    ];
    text = ''
      # Get list of clipboard items from CopyQ
      # Format: line_number: content_preview
      selected=$(copyq eval '
        var result = [];
        for (var i = 0; i < size(); ++i) {
          var text = read(i).toString().replace(/\n/g, " ");
          result.push(i + ": " + text);
        }
        print(result.join("\n"));
      ' | rofi -dmenu -p "󰕛  " -i)

      if [ -n "$selected" ]; then
        # Extract row index
        index=$(echo "$selected" | cut -d':' -f1)
        # Select and copy item to active clipboard
        copyq select "$index"
      fi
    '';
  };
in
{
  services.copyq.enable = true;
  home.packages = with pkgs; [
    copyq
    rofi-copyq
  ];
}
