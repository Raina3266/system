# Yazi keymap configuration.
# Plugin key bindings using chords to avoid shadowing single-key defaults.
{
  mgr.prepend_keymap = [
    # Override Enter key to show "open with" menu instead of direct open
    {
      on = "<Enter>";
      run = "open --interactive";
      desc = "Open with...";
    }
    # bookmarks — jump to a bookmark
    {
      on = "B";
      run = "plugin bookmarks jump";
      desc = "Jump to bookmark";
    }
    # bookmarks — delete a bookmark
    {
      on = [
        "b"
        "d"
      ];
      run = "plugin bookmarks delete";
      desc = "Delete bookmark";
    }
    # bookmarks — add a bookmark
    {
      on = [
        "b"
        "b"
      ];
      run = "plugin bookmarks save";
      desc = "Save bookmark";
    }
    # mount — disk mount manager
    {
      on = "M";
      run = "plugin mount";
      desc = "Mount manager";
    }
    # yafg — fuzzy grep file contents
    {
      on = [
        "F"
        "G"
      ];
      run = "plugin yafg";
      desc = "Grep file contents (rg+fzf)";
    }
    # recycle-bin — trash menu
    {
      on = [
        "R"
        "b"
      ];
      run = "plugin recycle-bin";
      desc = "Open Recycle Bin menu";
    }
    # compress — archive selected files
    {
      on = [
        "c"
        "a"
        "a"
      ];
      run = "plugin compress";
      desc = "Archive selected files";
    }
    {
      on = [
        "c"
        "a"
        "p"
      ];
      run = "plugin compress -p";
      desc = "Archive (password)";
    }
    {
      on = [
        "c"
        "a"
        "h"
      ];
      run = "plugin compress -ph";
      desc = "Archive (password+header)";
    }
    {
      on = [
        "c"
        "a"
        "l"
      ];
      run = "plugin compress -l";
      desc = "Archive (compression level)";
    }
    # chmod — change file mode
    {
      on = [
        "c"
        "m"
      ];
      run = "plugin chmod";
      desc = "Chmod on selected files";
    }
  ];
}