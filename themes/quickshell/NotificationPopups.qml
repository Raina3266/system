import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland
import "shared" as Shared

PanelWindow {
    id: window
    required property var center
    readonly property var targetScreen: Quickshell.screens.length > 0 ? Quickshell.screens[0] : null

    screen: targetScreen
    anchors { top: true; right: true }
    margins { top: 46; right: 6 }
    implicitWidth: Math.min(420, targetScreen ? targetScreen.width - 12 : 420)
    implicitHeight: Math.min(stack.implicitHeight + 12, targetScreen ? targetScreen.height - 72 : 800)
    visible: center.popups.length > 0 && targetScreen !== null
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    WlrLayershell.namespace: "quickshell:notifications"

    ScrollView {
        id: viewport
        anchors.fill: parent
        anchors.margins: 6
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        Column {
            id: stack
            width: viewport.availableWidth
            spacing: 6

            Repeater {
                model: window.center.popups

                delegate: Shared.PopupFrame {
                    id: card
                    required property var modelData
                    width: stack.width
                    implicitHeight: entry.implicitHeight + 16
                    borderColor: modelData.urgency === 2 ? colors.destructive : colors.warning

                    NotificationEntry {
                        id: entry
                        x: 8
                        y: 8
                        width: parent.width - 16
                        popup: true
                        notification: card.modelData
                        panel: window.center
                        onHoveredChanged: window.center.setHovered(notification.id, hovered)
                        Component.onDestruction: window.center.setHovered(notification.id, false)
                    }
                }
            }
        }
    }
}
