#!/usr/bin/env bash
# Fullscreen file browser with a large live preview pane.
# Uses rofi's recursivebrowser + the dedicated filepreview theme.
exec rofi -show recursivebrowser -theme ~/.config/rofi/filepreview.rasi
