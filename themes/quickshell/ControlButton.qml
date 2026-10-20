import QtQuick
import QtQuick.Controls

// Shared presentation only; callers own actions and checked state.
ToolButton {
    id: button

    readonly property var colors: Colors {}
    property string glyph: ""
    property int size: 32
    property bool outlined: true
    property bool circular: false
    property bool tabStyle: false
    // ToolButton already provides the writable highlighted property.
    property bool accent: checked || highlighted

    signal activated()
    onClicked: activated()

    implicitWidth: circular && text === "" ? size : contentItem.implicitWidth + 16
    implicitHeight: size
    padding: 0
    hoverEnabled: true

    contentItem: Item {
        implicitWidth: (icon.visible ? icon.implicitWidth : 0)
            + (label.visible ? label.implicitWidth : 0)
            + (icon.visible && label.visible ? 6 : 0)
        implicitHeight: Math.max(icon.implicitHeight, label.implicitHeight)

        Text {
            id: icon
            visible: text !== ""
            text: button.glyph
            x: label.visible ? 8 : (parent.width - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            color: button.enabled ? button.colors.destructive : button.colors.muted
            font.pixelSize: button.size * 0.5
            Behavior on color { ColorAnimation { duration: 120 } }
        }
        Text {
            id: label
            visible: text !== ""
            text: button.text
            x: button.tabStyle ? 0 : icon.visible ? icon.x + icon.width + 6 : 8
            width: button.tabStyle ? parent.width : Math.max(0, parent.width - x - 8)
            anchors.verticalCenter: parent.verticalCenter
            horizontalAlignment: button.tabStyle ? Text.AlignHCenter : Text.AlignLeft
            elide: Text.ElideRight
            textFormat: Text.PlainText
            color: !button.enabled ? button.colors.muted
                : button.tabStyle && (button.accent || button.hovered) ? button.colors.accent
                : button.tabStyle ? button.colors.primary
                : button.colors.accent
            font.pixelSize: button.tabStyle ? 13 : 12
            font.bold: true
            font.letterSpacing: button.tabStyle ? 1 : 0
            Behavior on color { ColorAnimation { duration: 120 } }
        }
    }

    background: Rectangle {
        radius: button.circular ? height / 2 : 3
        color: !button.enabled ? "transparent"
            : button.tabStyle
                ? (button.down ? Qt.alpha(button.colors.accent, 0.18)
                    : button.accent ? Qt.alpha(button.colors.accent, 0.08)
                    : button.hovered ? Qt.alpha(button.colors.primary, 0.1) : "transparent")
            : button.down ? Qt.alpha(button.colors.primary, 0.45)
            : button.hovered || button.visualFocus ? Qt.alpha(button.colors.primary, 0.333)
            : button.accent ? Qt.alpha(button.colors.dimPrimary, 0.35) : "transparent"
        border.pixelAligned: false
        border.width: button.tabStyle
            ? 0
            : button.outlined ? (button.enabled && button.hovered ? 1.5 : 0.35) : 0
        border.color: button.tabStyle
            ? (button.accent ? button.colors.accent : Qt.alpha(button.colors.primary, button.hovered ? 1 : 0.7))
            : button.enabled && button.hovered
                ? button.colors.warning
                : Qt.alpha(button.colors.text, button.enabled ? 0.5 : 0.25)
        Rectangle {
            visible: button.tabStyle
            width: parent.width
            height: button.accent ? 1.5 : 1
            color: button.accent ? button.colors.accent
                : Qt.alpha(button.colors.primary, button.hovered ? 1 : 0.7)
        }
        Rectangle {
            visible: button.tabStyle
            width: button.accent ? 1.5 : 1
            height: parent.height
            color: button.accent ? button.colors.accent
                : Qt.alpha(button.colors.primary, button.hovered ? 1 : 0.7)
        }
        Rectangle {
            visible: button.tabStyle
            x: parent.width - width
            width: button.accent ? 1.5 : 1
            height: parent.height
            color: button.accent ? button.colors.accent
                : Qt.alpha(button.colors.primary, button.hovered ? 1 : 0.7)
        }
        Rectangle {
            visible: button.tabStyle && !button.accent
            y: parent.height - height
            width: parent.width
            height: 1
            color: Qt.alpha(button.colors.primary, button.hovered ? 1 : 0.7)
        }
        Behavior on color { ColorAnimation { duration: 120 } }
        Behavior on border.width { NumberAnimation { duration: 100 } }
        Behavior on border.color { ColorAnimation { duration: 120 } }
    }
}
