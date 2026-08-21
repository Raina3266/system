# ~/GoogleDrive <-> the whole Drive, minus Obsidian + Music + Video + Trash
# ~/Music <-> GoogleDrive:Music
#
# Newer wins on conflicts; the loser is kept with a .conflict suffix. Routine
# overwrites and deletes do not create timestamped backup copies. Google Drive
# deletions use Drive's native Trash.
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
    description = "bisync GoogleDrive";
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
    description = "bisync oogleDrive:Music";
  }
  
  {
    name = "Onedrive";
    localDir = "${config.home.homeDirectory}/Onedrive";
    remote = "Onedrive:";
    description = "bisync Onedrive";
    excludes = [
      "Scans/**"
      "Attachments/**"
    ];
  }
]
