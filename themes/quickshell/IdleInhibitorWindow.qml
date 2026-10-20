import QtQuick
import Quickshell
import Quickshell.Wayland

// A one-pixel, click-through-free layer surface keeps Quickshell's native
// idle inhibitor alive after the dashboard closes without covering the screen.
PanelWindow {
    id: window

    required property var panel

    visible: panel.idleInhibitEnabled
    screen: Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
    implicitWidth: 1
    implicitHeight: 1
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    IdleInhibitor {
        window: window
        enabled: window.panel.idleInhibitEnabled
    }
}
