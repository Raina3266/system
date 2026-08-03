# ~/GoogleDrive <-> the whole Drive, minus Obsidian + Music + Video + Trash
# ~/Music <-> GoogleDrive:Music
# 
# Newer wins on conflicts; the loser is kept with a .conflict suffix. Files that
# lose a sync are soft-deleted into a local trash, purged after 30 days.
# Remote-side overwrites fall back to Drive's own trash.
#
# status:       systemctl --user status rclone-bisync-drive.service
# force resync: rm -rf ~/.local/state/rclone-bisync-drive
#               && systemctl --user start rclone-bisync-drive.service
# 
# status:       systemctl --user status rclone-bisync-music.service
# force resync: rm -rf ~/.local/state/rclone-bisync-music
#               && systemctl --user start rclone-bisync-music.service
{ config }:
[
  {
    name = "Gdrive";
    localDir = "${config.home.homeDirectory}/GoogleDrive";
    remote = "GoogleDrive:";
    description = "bisync ~/GoogleDrive with GoogleDrive";
    timeout = "6h";
    excludes = [
      "Shared/**"
      "Video/**"
      "Music/**"
      "Obsidian/**"
      ".Trash-1000/**"
    ];
  }

  {
    name = "Music";
    localDir = "${config.home.homeDirectory}/Music";
    remote = "GoogleDrive:Music";
    description = "bisync ~/Music with GoogleDrive:Music";
  }
]
