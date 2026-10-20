import QtQuick
import QtQuick.Controls

// Presentation only: the caller owns range, value and backend interaction.
Slider {
    id: slider

    readonly property var colors: Colors {}
    property color trackColor: colors.dimPrimary
    property bool animateChanges: true
    implicitHeight: 16

    background: Rectangle {
        x: slider.leftPadding
        y: slider.topPadding + slider.availableHeight / 2 - height / 2
        width: slider.availableWidth
        height: 4
        radius: 2
        color: slider.colors.secondaryBackground

        Rectangle {
            width: slider.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: slider.trackColor
            Behavior on width {
                enabled: slider.animateChanges && !slider.pressed
                NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }
    }

    handle: Rectangle {
        x: slider.leftPadding + slider.visualPosition * (slider.availableWidth - width)
        y: slider.topPadding + slider.availableHeight / 2 - height / 2
        width: 10
        height: 10
        radius: 5
        color: slider.colors.primary
        visible: slider.enabled
        scale: slider.pressed ? 1.35 : (slider.hovered ? 1.15 : 1.0)
        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
}
