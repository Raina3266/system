# Yazi keymap — plugin bindings use chords to avoid shadowing defaults.
{
  mgr.prepend_keymap = [
    { on = "<Enter>"; run = "open --interactive"; desc = "Open with..."; }

    # Bookmarks
    { on = "B"; run = "plugin bookmarks jump"; desc = "Jump to bookmark"; }
    { on = [ "b" "b" ]; run = "plugin bookmarks save"; desc = "Save bookmark"; }
    { on = [ "b" "d" ]; run = "plugin bookmarks delete"; desc = "Delete bookmark"; }

    # Bulk rename in GUI editor (Zed)
    { on = [ "r" "g" ]; run = "plugin batch-rename-gui"; desc = "Bulk rename in Zed"; }

    # Search
    { on = "M"; run = "plugin mount"; desc = "Mount manager"; }
    { on = [ "F" "G" ]; run = "plugin yafg"; desc = "Grep file contents (rg+fzf)"; }
    { on = [ "R" "b" ]; run = "plugin recycle-bin"; desc = "Open Recycle Bin menu"; }

    # Archives & permissions
    { on = [ "c" "a" "a" ]; run = "plugin compress"; desc = "Archive selected files"; }
    { on = [ "c" "a" "p" ]; run = "plugin compress -p"; desc = "Archive (password)"; }
    { on = [ "c" "a" "h" ]; run = "plugin compress -ph"; desc = "Archive (password+header)"; }
    { on = [ "c" "a" "l" ]; run = "plugin compress -l"; desc = "Archive (compression level)"; }
    { on = [ "c" "m" ]; run = "plugin chmod"; desc = "Chmod on selected files"; }
    { on = "C"; run = "plugin ouch"; desc = "Compress with ouch"; }

    # Paste
    { on = "p"; run = "plugin smart-paste"; desc = "Paste into hovered dir or CWD"; }
    { on = "P"; run = "plugin smart-paste --force"; desc = "Paste (overwrite)"; }

    # Selection
    { on = "<C-a>"; run = "select --all"; desc = "Select all files in cwd"; }

    # Navigation
    { on = "i"; run = "plugin easyjump"; desc = "Easyjump to visible file"; }
    { on = "f"; run = "plugin smart-filter"; desc = "Smart filter"; }
    { on = "<A-f>"; run = "plugin jump-to-char"; desc = "Jump to char"; }

    # Preview pane
    { on = "T"; run = "plugin toggle-pane min-preview"; desc = "Show/hide preview pane"; }
    { on = [ "T" "T" ]; run = "plugin toggle-pane max-preview"; desc = "Maximize/restore preview"; }
    { on = "+"; run = "plugin zoom 1"; desc = "Zoom in preview"; }
    { on = "-"; run = "plugin zoom -1"; desc = "Zoom out preview"; }
    { on = "<A-m>"; run = "plugin mediainfo -- toggle-metadata"; desc = "Toggle media metadata"; }
    { on = "<A-M>"; run = "plugin mediainfo -- toggle-preview"; desc = "Toggle media image"; }

    # Media / files
    { on = [ "c" "i" "p" ]; run = "plugin convert -- --extension='png'"; desc = "Convert to PNG"; }
    { on = [ "c" "i" "j" ]; run = "plugin convert -- --extension='jpg'"; desc = "Convert to JPG"; }
    { on = [ "c" "i" "w" ]; run = "plugin convert -- --extension='webp'"; desc = "Convert to WebP"; }
    { on = [ "R" "s" ]; run = "plugin rsync -- --remember"; desc = "Copy with rsync"; }
    { on = [ "<C-d>" "d" ]; run = "plugin diff"; desc = "Diff selected vs hovered"; }

    # Sudo operations (under <C-r>)
    { on = [ "<C-r>" "p" ]; run = "plugin sudo -- paste"; desc = "sudo paste"; }
    { on = [ "<C-r>" "P" ]; run = "plugin sudo -- paste --force"; desc = "sudo paste (overwrite)"; }
    { on = [ "<C-r>" "r" ]; run = "plugin sudo -- rename"; desc = "sudo rename"; }
    { on = [ "<C-r>" "l" ]; run = "plugin sudo -- link"; desc = "sudo symlink (absolute)"; }
    { on = [ "<C-r>" "L" ]; run = "plugin sudo -- link --relative"; desc = "sudo symlink (relative)"; }
    { on = [ "<C-r>" "H" ]; run = "plugin sudo -- hardlink"; desc = "sudo hardlink"; }
    { on = [ "<C-r>" "a" ]; run = "plugin sudo -- create"; desc = "sudo create"; }
    { on = [ "<C-r>" "d" ]; run = "plugin sudo -- remove"; desc = "sudo trash"; }
    { on = [ "<C-r>" "D" ]; run = "plugin sudo -- remove --permanently"; desc = "sudo delete"; }
    { on = [ "<C-r>" "m" ]; run = "plugin sudo -- chmod"; desc = "sudo chmod"; }
  ];
}
