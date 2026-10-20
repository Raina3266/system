import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Io
import Quickshell.Services.Pipewire
import "shared" as Shared

ColumnLayout {
    id: page
    readonly property var colors: Shared.Colors {}
    required property string kind
    required property string helper
    property bool active: false
    property var choices: []
    property string error: ""
    readonly property var node: kind === "output" ? Pipewire.defaultAudioSink : Pipewire.defaultAudioSource
    // Preserve the existing output boost range. PipeWire/Quickshell use 1 = 100%.
    readonly property real maximumVolume: kind === "output" ? 1.5 : 1.0
    readonly property var nativeDevices: Pipewire.nodes.values.filter(n => n.audio && !n.isStream && n.isSink === (kind === "output"))

    spacing: 10

    PwObjectTracker { objects: [page.node] }

    function refresh() {
        if (active && !listing.running && !selection.running)
            listing.running = true;
    }

    function select(entry) {
        error = "";
        const live = nativeDevices.find(n => n.name === entry.name);
        if (live && !entry.port) {
            if (kind === "output")
                Pipewire.preferredDefaultAudioSink = live;
            else
                Pipewire.preferredDefaultAudioSource = live;
        } else {
            // Native PipeWire has no physical port or inactive-profile API.
            selection.command = [helper, "set-default", kind, entry.key];
            selection.running = true;
        }
    }

    onActiveChanged: refresh()
    Connections {
        target: Pipewire
        function onDefaultAudioSinkChanged() { page.refresh(); }
        function onDefaultAudioSourceChanged() { page.refresh(); }
    }
    Connections {
        target: Pipewire.nodes
        function onValuesChanged() { page.refresh(); }
    }
    Timer {
        interval: 2000
        running: page.active
        repeat: true
        onTriggered: page.refresh()
    }
    Process {
        id: listing
        command: [page.helper, "devices", page.kind]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    page.choices = JSON.parse(text);
                    page.error = "";
                } catch (e) {
                    page.error = "Could not read audio devices.";
                }
            }
        }
        stderr: StdioCollector {
            onStreamFinished: { if (text.trim()) page.error = text.trim(); }
        }
    }
    Process {
        id: selection
        stderr: StdioCollector {
            onStreamFinished: { if (text.trim()) page.error = text.trim(); }
        }
        onExited: page.refresh()
    }

    Label {
        Layout.fillWidth: true
        text: page.node ? (page.node.nickname || page.node.description || page.node.name) : "No " + page.kind + " device"
        textFormat: Text.PlainText
        elide: Text.ElideRight
        font.bold: true
        color: page.colors.accent
    }
    RowLayout {
        Layout.fillWidth: true
        Shared.IconButton {
            outlined: false
            glyph: page.kind === "input"
                ? (page.node?.audio?.muted ? "󰍭" : "󰍬")
                : (page.node?.audio?.muted ? "󰝟" : "󰕾")
            enabled: !!page.node?.audio
            onActivated: page.node.audio.muted = !page.node.audio.muted
            ToolTip.visible: hovered
            ToolTip.text: page.node?.audio?.muted ? "Unmute" : "Mute"
        }
        Label {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignRight
            text: Math.round((page.node?.audio?.volume ?? 0) * 100) + "%"
            color: page.colors.accent
            font.bold: true
        }
    }
    Shared.StyledSlider {
        id: volume
        Layout.fillWidth: true
        from: 0
        to: page.maximumVolume
        stepSize: 0.01
        enabled: !!page.node?.audio
        value: page.node?.audio?.volume ?? 0
        onMoved: page.node.audio.volume = value
    }
    Label {
        Layout.fillWidth: true
        visible: page.kind === "output"
        text: "0–150% · above 100% amplifies audio"
        color: page.colors.muted
        font.pixelSize: 11
    }
    Label { text: "Devices"; font.bold: true; color: page.colors.primary }
    ScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        contentWidth: availableWidth
        ColumnLayout {
            width: parent.width
            spacing: 6
            Repeater {
                model: page.choices
                delegate: Shared.ControlButton {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    text: (modelData.default ? "✓  " : "") + modelData.label
                    highlighted: modelData.default
                    enabled: !selection.running
                    onActivated: page.select(modelData)
                    ToolTip.visible: hovered
                    ToolTip.text: modelData.description
                }
            }
            Label {
                Layout.fillWidth: true
                visible: page.choices.length === 0 && !listing.running
                text: "No available devices."
                color: page.colors.muted
            }
        }
    }
    Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: page.error
        textFormat: Text.PlainText
        color: page.colors.destructive
        wrapMode: Text.Wrap
    }
}
