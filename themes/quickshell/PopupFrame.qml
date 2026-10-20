import QtQuick
import QtQuick.Shapes

// Only the fill is translucent: place opaque content alongside this shape.
Shape {
    id: frame

    readonly property var colors: Colors {}
    property color backgroundColor: "#25252F"
    property real backgroundOpacity: 0.90
    property real cornerCut: 10
    property real borderWidth: 1
    property color borderColor: colors.warning
    readonly property real inset: borderWidth / 2
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
        strokeColor: frame.borderColor
        strokeWidth: frame.borderWidth
        fillColor: Qt.alpha(frame.backgroundColor, frame.backgroundOpacity)
        joinStyle: ShapePath.MiterJoin
        startX: frame.cornerCut + frame.inset
        startY: frame.inset
        PathLine { x: frame.width - frame.inset; y: frame.inset }
        PathLine { x: frame.width - frame.inset; y: frame.height - frame.cornerCut - frame.inset }
        PathLine { x: frame.width - frame.cornerCut - frame.inset; y: frame.height - frame.inset }
        PathLine { x: frame.inset; y: frame.height - frame.inset }
        PathLine { x: frame.inset; y: frame.cornerCut + frame.inset }
        PathLine { x: frame.cornerCut + frame.inset; y: frame.inset }
    }
}
