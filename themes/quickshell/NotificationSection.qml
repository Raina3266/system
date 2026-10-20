import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "shared" as Shared

DashboardSection {
    id: section

    required property var panel
    glyph: "󰂚"
    title: "NOTIFICATIONS"

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        Text {
            Layout.fillWidth: true
            text: "HISTORY · " + section.panel.notifications.length
            color: section.colors.muted
            font.pixelSize: 9
            font.bold: true
            font.letterSpacing: 0.8
            elide: Text.ElideRight
        }

        Shared.IconButton {
            glyph: section.panel.dndActive ? "󰂛" : "󰂚"
            size: 28
            onActivated: section.panel.toggleDnd()
            ToolTip.visible: hovered
            ToolTip.text: section.panel.dndActive ? "Disable do not disturb" : "Enable do not disturb"
        }
        Shared.IconButton {
            glyph: "󰆴"
            size: 28
            enabled: section.panel.notifications.length > 0
            onActivated: section.panel.clearNotifications()
            ToolTip.visible: hovered
            ToolTip.text: "Clear all"
        }
    }

    Text {
        Layout.fillWidth: true
        visible: section.panel.notifications.length === 0
        text: "NO RECENT ALERTS"
        color: section.colors.muted
        font.pixelSize: 10
        font.letterSpacing: 1.5
        horizontalAlignment: Text.AlignHCenter
        Layout.topMargin: 8
        Layout.bottomMargin: 8
    }

    ListView {
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(contentHeight, 3 * 78)
        visible: section.panel.notifications.length > 0
        model: section.panel.notifications
        spacing: 6
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        delegate: NotificationEntry {
            required property var modelData
            // ListView does not honor Layout.fillWidth on its delegates.
            width: ListView.view ? ListView.view.width : 0
            notification: modelData
            panel: section.panel
        }
    }
}
