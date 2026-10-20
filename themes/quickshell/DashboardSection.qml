import QtQuick
import QtQuick.Layouts
import "shared" as Shared

// Shared section framing: flat dark surface, cyan details, small corners.
Rectangle {
    id: section

    default property alias content: body.data
    readonly property var colors: Shared.Colors {}
    property string glyph: ""
    property string title: ""

    Layout.fillWidth: true
    implicitHeight: body.implicitHeight + 24
    radius: 3
    color: colors.elevated
    border.width: 1
    border.color: Qt.alpha(colors.accent, 0.28)

    ColumnLayout {
        id: body
        anchors.fill: parent
        anchors.margins: 12
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Text {
                text: section.glyph
                color: section.colors.primary
                font.pixelSize: 14
            }
            Text {
                Layout.fillWidth: true
                text: section.title
                color: section.colors.accent
                font.pixelSize: 10
                font.bold: true
                font.letterSpacing: 2
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Qt.alpha(section.colors.accent, 0.16)
        }
    }
}
