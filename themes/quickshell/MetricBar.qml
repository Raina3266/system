import QtQuick
import QtQuick.Layouts
import "shared" as Shared

ColumnLayout {
    id: metric

    readonly property var colors: Shared.Colors {}
    property string label: ""
    property string value: "—"
    property real progress: 0

    Layout.fillWidth: true
    spacing: 4

    RowLayout {
        Layout.fillWidth: true
        Text {
            text: metric.label
            color: metric.colors.muted
            font.pixelSize: 10
            font.bold: true
            font.letterSpacing: 1.2
        }
        Item { Layout.fillWidth: true }
        Text {
            text: metric.value
            color: metric.colors.text
            font.pixelSize: 11
            font.family: "JetBrains Mono"
            horizontalAlignment: Text.AlignRight
        }
    }

    Rectangle {
        Layout.fillWidth: true
        height: 5
        radius: 2
        color: Qt.alpha(metric.colors.muted, 0.2)
        clip: true

        Rectangle {
            width: parent.width * Math.max(0, Math.min(100, metric.progress)) / 100
            height: parent.height
            radius: 2
            color: metric.progress >= 90 ? metric.colors.destructive : metric.colors.primary
            Behavior on width { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
        }
    }
}
