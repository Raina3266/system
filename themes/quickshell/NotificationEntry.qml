import QtQuick
import QtQuick.Layouts
import "shared" as Shared

Rectangle {
    id: entry

    readonly property var colors: Shared.Colors {}
    required property var notification
    required property var panel
    property bool popup: false
    readonly property bool hovered: pointer.containsMouse
    readonly property var actions: (notification.actions || []).filter(action => action.identifier !== "default")

    implicitHeight: Math.max(64, copy.implicitHeight + 18)
    radius: 2
    color: popup ? "transparent" : (hovered ? Qt.alpha(colors.dimPrimary, 0.35) : colors.secondaryBackground)
    border.width: popup ? 0 : 1
    border.color: Qt.alpha(colors.accent, 0.42)
    clip: true

    Behavior on color { ColorAnimation { duration: 120 } }

    MouseArea {
        anchors.fill: parent
        onClicked: {
            if ((entry.notification.actions || []).some(action => action.identifier === "default"))
                entry.panel.invokeNotificationAction(entry.notification.id, "default")
        }
    }

    Rectangle {
        width: 2
        anchors { top: parent.top; bottom: parent.bottom; left: parent.left }
        color: entry.notification.urgency === 2 ? entry.colors.destructive : entry.colors.accent
    }

    Image {
        id: artwork
        x: 12
        y: 9
        width: 32
        height: 32
        source: entry.notification.icon || ""
        visible: source.toString() !== ""
        fillMode: Image.PreserveAspectFit
    }

    ColumnLayout {
        id: copy
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 9; leftMargin: artwork.visible ? 52 : 12; rightMargin: 34 }
        spacing: 3

        Text {
            Layout.fillWidth: true
            text: (entry.notification.app || "SYSTEM").toUpperCase()
            textFormat: Text.PlainText
            color: entry.colors.primary
            font.pixelSize: 9
            font.bold: true
            font.letterSpacing: 1.2
            elide: Text.ElideRight
        }
        Text {
            Layout.fillWidth: true
            text: entry.notification.summary || "Notification"
            textFormat: Text.PlainText
            color: entry.colors.accent
            font.pixelSize: 12
            font.bold: true
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
        }
        Text {
            Layout.fillWidth: true
            visible: entry.notification.body !== ""
            text: entry.notification.body || ""
            color: entry.colors.text
            font.pixelSize: 10
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            maximumLineCount: 3
            elide: Text.ElideRight
        }

        Flow {
            Layout.fillWidth: true
            visible: entry.actions.length > 0
            spacing: 6

            Repeater {
                model: entry.actions
                delegate: Shared.ControlButton {
                    required property var modelData
                    text: modelData.text
                    size: 28
                    width: Math.min(implicitWidth, parent.width)
                    onActivated: entry.panel.invokeNotificationAction(entry.notification.id, modelData.identifier)
                }
            }
        }
    }

    Shared.IconButton {
        anchors { top: parent.top; right: parent.right; topMargin: 4; rightMargin: 3 }
        glyph: "󰅖"
        size: 26
        onActivated: entry.panel.dismissNotification(entry.notification.id)
    }

    MouseArea {
        id: pointer
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.NoButton
    }
}
