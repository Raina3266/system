import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import QtQuick.Layouts
import "shared" as Shared

// The dashboard uses the media card's dimensions, flat frame, and neon palette.
Item {
    id: dashboard

    required property var panel
    required property bool active
    readonly property var colors: Shared.Colors {}

    implicitWidth: 440
    implicitHeight: contentColumn.implicitHeight + 32

    MouseArea {
        anchors.fill: parent
        onClicked: { }
    }

    Shared.PopupFrame {
        id: frame
        anchors.fill: parent
        backgroundColor: dashboard.colors.background
        backgroundOpacity: 0.9
        cornerCut: 10
        borderWidth: 1
        borderColor: dashboard.colors.warning
    }

    MultiEffect {
        anchors.fill: frame
        source: frame
        shadowEnabled: true
        shadowColor: Qt.alpha(dashboard.colors.warning, 0.65)
        shadowBlur: 0.65
        shadowOpacity: 0.6
        shadowVerticalOffset: 3
    }

    SystemMetrics {
        id: metrics
        active: dashboard.active
    }

    ScrollView {
        id: scroll
        anchors.fill: parent
        anchors.margins: 16
        clip: true
        ScrollBar.vertical.policy: ScrollBar.AsNeeded

        ColumnLayout {
            id: contentColumn
            width: scroll.availableWidth
            spacing: 10

            PowerSection {
                panel: dashboard.panel
            }

            SystemSection {
                metrics: metrics
            }

            NotificationSection {
                panel: dashboard.panel
            }
        }
    }
}
