import QtQuick
import QtQuick.Layouts
import "shared" as Shared

// The output-device dropdown: one row per `wayle-list output` choice,
// including inactive-profile rows (laptop Speaker/Headphones) that have no
// live sink name yet. Same row structure as the media dropdown: device icon,
// label over description, current row highlighted and bold.
ColumnLayout {
    id: list

    readonly property var colors: Shared.Colors {}
    required property color detailColor
    property var routes: []
    property string currentName: ""
    property bool open: false

    signal selected(string key)

    visible: open
    opacity: open ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 140 } }
    spacing: 2

    function iconFor(label) {
        const lower = label.toLowerCase()
        if (lower.indexOf("headphone") !== -1 || lower.indexOf("headset") !== -1 || lower.indexOf("buds") !== -1)
            return "󰋋"
        if (lower.indexOf("hdmi") !== -1 || lower.indexOf("displayport") !== -1)
            return "󰍹"
        if (lower.indexOf("bluetooth") !== -1 || lower.indexOf("airpods") !== -1)
            return "󰂯"
        return "󰓃"
    }

    Repeater {
        model: list.routes

        delegate: Rectangle {
            required property var modelData

            Layout.fillWidth: true
            implicitHeight: 42
            radius: 4
            readonly property bool current: modelData.name !== "" && modelData.name === list.currentName
            color: current ? list.colors.dimPrimary : (area.containsMouse ? Qt.alpha(list.colors.dimPrimary, 0.5) : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            MouseArea {
                id: area
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: list.selected(modelData.key)
            }

            RowLayout {
                anchors { fill: parent; leftMargin: 10; rightMargin: 8 }
                spacing: 8

                Text {
                    Layout.preferredWidth: 20
                    text: list.iconFor(modelData.label)
                    color: list.colors.primary
                    font.pixelSize: 15
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1

                    Text {
                        Layout.fillWidth: true
                        text: modelData.label
                        color: list.colors.accent
                        font.pixelSize: 13
                        font.bold: true
                        elide: Text.ElideRight
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: modelData.description !== modelData.label
                        text: modelData.description
                        color: list.detailColor
                        font.pixelSize: 11
                        elide: Text.ElideRight
                    }
                }
                Text {
                    visible: modelData.isDefault
                    text: "DEFAULT"
                    color: list.colors.muted
                    font.pixelSize: 9
                    font.letterSpacing: 1.5
                }
            }
        }
    }
}
