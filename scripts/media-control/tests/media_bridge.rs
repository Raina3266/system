//! Exercise the real QML bridges without connecting them to desktop services.
//! QML is deliberately read at runtime: Nix's filtered Rust sources omit it.

use std::env;
use std::fs::{self, DirBuilder};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        for _ in 0..100 {
            let path = env::temp_dir().join(format!(
                "media-control-bridge-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create {}: {error}", path.display()),
            }
        }
        panic!("could not allocate a media bridge fixture directory");
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn skip(name: &str, reason: impl std::fmt::Display) {
    // Write directly so the skip remains visible even with libtest's capture.
    let _ = writeln!(io::stderr().lock(), "SKIP media_bridge::{name}: {reason}");
}

fn tool(name: &str, fixture: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| {
                fs::metadata(candidate).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            })
    });
    if path.is_none() {
        skip(fixture, format!("{name} is not available in PATH"));
    }
    path
}

fn run_fixture(name: &str, components: &[&str], imports: &str, body: &str) {
    let Some(dbus) = tool("dbus-run-session", name) else {
        return;
    };
    let Some(timeout) = tool("timeout", name) else {
        return;
    };
    let Some(quickshell) = tool("quickshell", name) else {
        return;
    };

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/quickshell");
    let mut sources = Vec::new();
    for component in components {
        let path = source.join(component.strip_prefix("shared/").unwrap_or(component));
        match fs::read_to_string(&path) {
            Ok(text) => sources.push((component, text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                skip(name, format!("runtime QML is absent: {}", path.display()));
                return;
            }
            Err(error) => panic!("read {}: {error}", path.display()),
        }
    }

    let directory = FixtureDir::new();
    let directory = &directory.0;
    for child in ["runtime", "cache", "config", "data", "state", "home"] {
        DirBuilder::new()
            .mode(0o700)
            .create(directory.join(child))
            .unwrap();
    }
    for (component, text) in sources {
        let path = directory.join(component);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    if components
        .iter()
        .any(|component| component.starts_with("shared/"))
    {
        let properties = [
            "background",
            "secondaryBackground",
            "surface",
            "elevated",
            "chrome",
            "text",
            "muted",
            "accent",
            "metadata",
            "primary",
            "dimPrimary",
            "destructive",
            "warning",
            "ok",
        ]
        .map(|name| format!("property color {name}: 'gray'"))
        .join("; ");
        fs::write(
            directory.join("shared/Colors.qml"),
            format!("import QtQuick\nQtObject {{ {properties} }}"),
        )
        .unwrap();
    }
    let helper = env!("CARGO_BIN_EXE_media-control");
    assert!(
        Path::new(helper).is_file(),
        "missing Cargo helper: {helper}"
    );
    let shell = HARNESS
        .replace("@IMPORTS@", imports)
        .replace("@BODY@", body)
        .replace("@HELPER@", &serde_json::to_string(helper).unwrap())
        .replace("@NAME@", &serde_json::to_string(name).unwrap());
    fs::write(directory.join("shell.qml"), shell).unwrap();

    // The worker only receives model/credit requests. Never open
    // the panel or request a move; private sockets also fence off live services.
    let output = Command::new(dbus)
        .arg("--")
        .arg(timeout)
        .args(["--kill-after=2s", "15s"])
        .arg(quickshell)
        .arg("-p")
        .arg(directory.join("shell.qml"))
        .arg("--no-color")
        .current_dir(directory)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_BACKEND", "software")
        .env("QML_DISABLE_DISK_CACHE", "1")
        .env("NO_AT_BRIDGE", "1")
        .env("XDG_RUNTIME_DIR", directory.join("runtime"))
        .env("XDG_CACHE_HOME", directory.join("cache"))
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("XDG_DATA_HOME", directory.join("data"))
        .env("XDG_STATE_HOME", directory.join("state"))
        .env("HOME", directory.join("home"))
        .env("PIPEWIRE_REMOTE", "media-bridge-no-pipewire")
        .env(
            "PULSE_SERVER",
            format!("unix:{}", directory.join("no-pulse").display()),
        )
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            format!("unix:path={}", directory.join("no-system-bus").display()),
        )
        .env("MEDIA_CONTROL_CURL", directory.join("no-network-helper"))
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DBUS_STARTER_ADDRESS")
        .env_remove("DBUS_STARTER_BUS_TYPE")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env_remove("QT_LOGGING_RULES")
        .output()
        .expect("run the bounded, isolated Quickshell fixture");
    let transcript = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics: Vec<_> = transcript
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            line.starts_with("error")
                || line.contains(" error:")
                || (line.contains("warn") && line.contains("qml"))
                || [
                    "media_bridge_fail",
                    "referenceerror:",
                    "typeerror:",
                    "syntaxerror:",
                    "rangeerror:",
                    "binding loop",
                    "cannot assign",
                    "unable to assign",
                    "is not a type",
                    "is not installed",
                    "failed to load configuration",
                    "error loading qml",
                    "media helper:",
                    "media credit:",
                    "media panel: invalid",
                ]
                .iter()
                .any(|pattern| line.contains(pattern))
        })
        .collect();
    assert!(
        output.status.success()
            && transcript.contains(&format!("MEDIA_BRIDGE_PASS {name}"))
            && diagnostics.is_empty(),
        "{name}: status {}; diagnostics: {diagnostics:?}\n{transcript}",
        output.status
    );
}

const HARNESS: &str = r#"
import QtQuick
import Quickshell
@IMPORTS@

ShellRoot {
    id: root
    property string helper: @HELPER@
    property string fixtureName: @NAME@
    property bool finished: false
    property int phase: 0

    function check(condition, message) {
        if (!condition)
            throw new Error(message)
    }
    function fail(message) {
        if (finished)
            return
        finished = true
        console.error("MEDIA_BRIDGE_FAIL " + fixtureName + ": " + message)
        Qt.exit(1)
    }
    function pass() {
        finished = true
        console.log("MEDIA_BRIDGE_PASS " + fixtureName)
        Qt.quit()
    }
    Timer {
        interval: 10
        repeat: true
        running: !root.finished
        onTriggered: {
            try {
                root.advance()
            } catch (error) {
                root.fail("phase " + root.phase + ": " + String(error))
            }
        }
    }
    Timer {
        interval: 11000
        running: !root.finished
        onTriggered: root.fail("watchdog expired at phase " + root.phase)
    }

@BODY@
}
"#;

const NATIVE_FIXTURE: &str = r#"
    component FakePlayer: QtObject {
        property string dbusName: ""
        property string identity: "Bridge Player"
        property string desktopEntry: "bridge-player"
        property string trackTitle: "Fixture \"track\" • A"
        property string trackArtist: "Fixture artist"
        property string trackAlbum: "Fixture album"
        property string trackArtUrl: "file:///media-bridge/cover.png"
        property var metadata: ({ "xesam:url": "file:///media-bridge/track-a.ogg" })
        property bool isPlaying: true
        property int playbackState: isPlaying ? MprisPlaybackState.Playing : MprisPlaybackState.Paused
        property real position: 20
        property real length: 180
        property bool lengthSupported: true
        property bool positionSupported: true
        property real rate: 1
        property bool canSeek: true
        property bool canGoNext: true
        property bool canGoPrevious: true
        property int playCalls: 0
        property int pauseCalls: 0
        property int nextCalls: 0
        property int previousCalls: 0
        property int positionWrites: 0
        onPositionChanged: positionWrites++
        function play() { playCalls++; isPlaying = true }
        function pause() { pauseCalls++; isPlaying = false }
        function next() { nextCalls++ }
        function previous() { previousCalls++ }
    }
    FakePlayer { id: publisherA; dbusName: "org.mpris.MediaPlayer2.bridgeA" }
    FakePlayer { id: publisherB; dbusName: "org.mpris.MediaPlayer2.bridgeB"; position: 21 }
    FakePlayer {
        id: otherPlayer
        dbusName: "org.mpris.MediaPlayer2.other"
        identity: "Other Player"
        desktopEntry: "other-player"
        trackTitle: "Other track"
        metadata: ({ "xesam:url": "file:///media-bridge/other.ogg" })
        isPlaying: false
        position: 5
        canGoNext: false
    }
    QtObject {
        id: fakeMpris
        property QtObject players: QtObject {
            property var values: [publisherA, publisherB, otherPlayer]
        }
    }
    component FakeNode: QtObject {
        property int id: 0
        property string name: ""
        property string nickname: ""
        property bool isStream: true
        property bool isSink: true
        property bool audio: true
        property bool ready: true
        property var properties: ({})
    }

"#;

const PIPEWIRE_FIXTURE: &str = r#"
    FakeNode {
        id: stream
        name: "bridge-player"
        properties: ({ "object.serial": "4242", "application.id": "bridge-player" })
    }
    FakeNode { id: sink; name: "fixture-sink"; isStream: false }
    FakeNode { id: inputStream; name: "ignored-recording"; isSink: false }
    QtObject {
        id: link
        property QtObject source: stream
        property QtObject target: sink
    }
    QtObject {
        id: fakePipewire
        property QtObject nodes: QtObject {
            property var values: [stream, sink, inputStream]
        }
        property QtObject linkGroups: QtObject {
            property var values: [link]
        }
    }
    MediaController {
        id: controller
        helper: root.helper
        mpris: fakeMpris
        pipewire: fakePipewire
        opened: false
        trackNativeObjects: false
        onMovingChanged: {
            if (moving)
                root.fail("unexpected stream-move invocation")
        }
    }
    function idle() {
        return controller.workerReady && controller.inFlight === null
            && !controller.refreshPending && controller.queuedActions.length === 0
    }
"#;

const MODEL_ROUNDTRIP: &str = r#"
    property var seekWrites: []
    function advance() {
        if (!idle())
            return
        check(controller.failure === "", "worker failure: " + controller.failure)
        check(!controller.moving && controller.routes.length === 0,
            "a closed fixture must not discover devices or move streams")
        switch (phase) {
        case 0: {
            const snapshot = JSON.parse(JSON.stringify(controller.snapshot()))
            check(snapshot.players.length === 3 && snapshot.streams.length === 1,
                "snapshot must serialize fake players and only playback streams")
            check(snapshot.players[0].browserHint === null, "absent browser hint must survive JSON")
            check(controller.groups.length === 2, "duplicate publishers must form one of two groups")
            const group = controller.activeGroup
            check(group && group.title === publisherA.trackTitle && group.primary === publisherA,
                "worker must restore the active group's metadata and native primary")
            check(group.players.length === 2 && group.players[0] === publisherA
                && group.players[1] === publisherB, "group members must be native objects")
            check(group.streamNode === stream && group.selector === "serial:4242",
                "worker stream ownership must round-trip")
            check(controller.streamSink === sink && group.sinkName === "fixture-sink",
                "fake native link groups must restore the sink")
            controller.action("toggle", publisherB)
            break
        }
        case 1:
            check(publisherA.pauseCalls === 1 && publisherB.pauseCalls === 1
                && otherPlayer.pauseCalls === 0, "pause must fan out only to the requested group")
            check(!publisherA.isPlaying && !publisherB.isPlaying, "native pause was not applied")
            controller.action("toggle", publisherB)
            break
        case 2:
            check(publisherA.playCalls === 1 && publisherB.playCalls === 1
                && otherPlayer.playCalls === 0, "play must fan out to both publishers")
            check(publisherA.isPlaying && publisherB.isPlaying, "native play was not applied")
            controller.action("next", publisherB)
            break
        case 3:
            check(publisherA.nextCalls === 1 && publisherB.nextCalls === 0
                && otherPlayer.nextCalls === 0, "next must execute once, on the primary only")
            controller.action("previous", publisherB)
            break
        case 4:
            check(publisherA.previousCalls === 1 && publisherB.previousCalls === 0,
                "previous must execute only on the primary")
            seekWrites = [publisherA.positionWrites, publisherB.positionWrites, otherPlayer.positionWrites]
            controller.action("seek", publisherB, 42.25)
            break
        case 5:
            check(publisherA.position === 42.25 && publisherB.position === 42.25
                && otherPlayer.position === 5, "seek must write the native group positions")
            check(publisherA.positionWrites === seekWrites[0] + 1
                && publisherB.positionWrites === seekWrites[1] + 1
                && otherPlayer.positionWrites === seekWrites[2], "seek property-write counters differ")
            controller.selectGroup(controller.groups.find(group => group.primary === otherPlayer))
            break
        case 6:
            check(controller.activeGroup.primary === otherPlayer, "selection must survive the worker reply")
            controller.action("next", otherPlayer)
            break
        case 7:
            check(otherPlayer.nextCalls === 0, "an incapable native primary must not receive next")
            fakeMpris.players.values = [publisherA, otherPlayer]
            break
        case 8: {
            const group = controller.groups.find(group => group.primary === publisherA)
            check(controller.groups.length === 2 && group.players.length === 1,
                "the injectable players model must trigger a fresh worker snapshot")
            controller.selectGroup(group)
            fakePipewire.linkGroups.values = []
            break
        }
        case 9:
            check(controller.activeGroup.primary === publisherA && controller.streamNode === stream
                && controller.streamSink === null && controller.activeGroup.sinkName === "",
                "link model changes must clear the adopted sink without losing the stream")
            pass()
            return
        }
        phase++
    }
"#;

const STALE_ACTIONS: &str = r#"
    function advance() {
        if (!idle())
            return
        switch (phase) {
        case 0: {
            check(controller.failure === "", "initial worker failure: " + controller.failure)
            const original = publisherA.trackTitle
            const stamp = controller.stamp(controller.snapshot())
            publisherA.position += 0.25
            publisherB.position += 0.25
            check(controller.stamp(controller.snapshot()) === stamp,
                "ordinary native position progress must not invalidate a plan")

            // Adopted groups still describe A. Queue against raw native B, then
            // return to A before dispatch: a guard derived from groups is wrong.
            controller.workerReady = false
            publisherA.trackTitle = "Unadopted track B"
            publisherB.trackTitle = "Unadopted track B"
            check(controller.activeGroup.title === original, "fixture unexpectedly adopted B")
            check(controller.snapshot().players[0].title === "Unadopted track B"
                && controller.stamp(controller.snapshot()) !== stamp,
                "the raw QML snapshot must observe the unadopted track")
            controller.action("next", publisherB)
            check(controller.queuedActions.length === 1, "action was not held before dispatch")
            publisherA.trackTitle = original
            publisherB.trackTitle = original
            controller.workerReady = true
            controller.refresh()
            break
        }
        case 1:
            check(publisherA.nextCalls === 0 && publisherB.nextCalls === 0,
                "a track change before queued dispatch must discard next")

            // Dispatch synchronously, then change identity before the event
            // loop can deliver the real worker reply. No sleeps or worker race.
            controller.action("next", publisherA)
            controller.pump()
            check(controller.inFlight !== null && controller.inFlight.request.action,
                "expected a real native action request in flight")
            publisherA.metadata = ({ "xesam:url": "file:///media-bridge/replacement.ogg" })
            publisherB.metadata = ({ "xesam:url": "file:///media-bridge/replacement.ogg" })
            break
        case 2:
            check(publisherA.nextCalls === 0 && publisherB.nextCalls === 0,
                "a track change after dispatch must discard the stale worker plan")
            controller.workerReady = false
            controller.action("next", publisherB)
            publisherA.position += 0.01
            publisherB.position += 0.01
            controller.workerReady = true
            break
        case 3:
            check(controller.failure === "", "valid action did not recover: " + controller.failure)
            check(publisherA.nextCalls === 1 && publisherB.nextCalls === 0
                && otherPlayer.nextCalls === 0,
                "a position-only change must allow exactly one next after stale actions")
            pass()
            return
        }
        phase++
    }
"#;

const CREDIT_ROUNDTRIPS: &str = r#"
    property string urlA: "file:///media-bridge/track-a.ogg"
    property string artistA: "Artist \"A\" 雪"
    property string albumA: "Album A"
    property string urlB: "file:///media-bridge/track-b.ogg"
    property string artistB: "Artist B"
    property string albumB: "Album B"
    property int quietTicks: 0
    MediaCredit {
        id: credit
        helper: root.helper
        active: true
        url: root.urlA
        artist: root.artistA
        album: root.albumA
    }
    function setTrack(url, artist, album) {
        credit.url = url
        credit.artist = artist
        credit.album = album
    }
    function hasCredit(artist, album) {
        check(credit.text === "" || credit.text === artist + " · " + album,
            "unexpected or stale local-file credit: " + credit.text)
        return credit.text === artist + " · " + album
    }
    function advance() {
        switch (phase) {
        case 0:
            if (!hasCredit(artistA, albumA))
                return
            credit.active = false
            setTrack("", "", "")
            quietTicks = 0
            break
        case 1:
            if (++quietTicks < 4)
                return
            check(!credit.active && credit.url === "", "expected an inactive no-player identity")
            setTrack(urlA, artistA, albumA)
            quietTicks = 0
            break
        case 2:
            if (++quietTicks < 4)
                return
            credit.active = true
            break
        case 3:
            if (!hasCredit(artistA, albumA))
                return
            credit.active = false
            setTrack(urlB, artistB, albumB)
            quietTicks = 0
            break
        case 4:
            if (++quietTicks < 4)
                return
            check(!credit.active && credit.url === urlB, "expected inactive track B")
            setTrack(urlA, artistA, albumA)
            quietTicks = 0
            break
        case 5:
            if (++quietTicks < 4)
                return
            credit.active = true
            break
        case 6:
            if (!hasCredit(artistA, albumA))
                return
            setTrack(urlB, artistB, albumB)
            break
        case 7:
            if (!hasCredit(artistB, albumB))
                return
            quietTicks = 0
            break
        case 8:
            if (++quietTicks < 4)
                return
            check(hasCredit(artistB, albumB), "final local-file credit disappeared")
            pass()
            return
        }
        phase++
    }
"#;

#[test]
fn media_controller_worker_roundtrip_and_native_controls() {
    run_fixture(
        "controller_roundtrip",
        &["MediaController.qml"],
        "import Quickshell.Services.Mpris",
        &[NATIVE_FIXTURE, PIPEWIRE_FIXTURE, MODEL_ROUNDTRIP].concat(),
    );
}

#[test]
fn media_controller_rejects_track_changes_before_and_after_dispatch() {
    run_fixture(
        "controller_stale_actions",
        &["MediaController.qml"],
        "import Quickshell.Services.Mpris",
        &[NATIVE_FIXTURE, PIPEWIRE_FIXTURE, STALE_ACTIONS].concat(),
    );
}

const QUEUED_SEEKS: &str = r#"
    function advance() {
        if (!idle())
            return
        if (phase === 0) {
            controller.action("seek", publisherA, 30)
            controller.pump()
            check(controller.inFlight !== null, "first seek was not dispatched")
            controller.action("seek", publisherA, 90)
            phase++
        } else {
            check(publisherA.position === 90 && publisherB.position === 90,
                "the final queued drag target must survive the preceding native seek")
            check(controller.failure === "", "valid queued seek was rejected: " + controller.failure)
            pass()
        }
    }
"#;

#[test]
fn media_controller_preserves_the_final_queued_seek() {
    run_fixture(
        "controller_queued_seeks",
        &["MediaController.qml"],
        "import Quickshell.Services.Mpris",
        &[NATIVE_FIXTURE, PIPEWIRE_FIXTURE, QUEUED_SEEKS].concat(),
    );
}

#[test]
fn media_controller_respects_native_timeline_support_and_rate() {
    run_fixture(
        "native_timeline",
        &["MediaController.qml"],
        "import Quickshell.Services.Mpris",
        &[NATIVE_FIXTURE, PIPEWIRE_FIXTURE, r#"
    function advance() {
        if (!idle())
            return
        otherPlayer.positionSupported = false
        otherPlayer.lengthSupported = false
        otherPlayer.length = otherPlayer.position
        publisherA.rate = 2
        publisherB.rate = 2
        const data = controller.snapshot()
        const pending = { data: data, stamp: controller.stamp(data),
            selection: controller.selectionSerial, time: Date.now() - 1000 }
        check(data.players[2].length === null, "unknown native duration must not become track identity")
        publisherA.position += 2
        publisherB.position += 2
        otherPlayer.position = 123
        otherPlayer.length = 123
        check(controller.current(pending), "native rate and unsupported timelines must not invalidate a plan")
        publisherA.position += 20
        check(!controller.current(pending), "an external seek must invalidate the old plan")
        pass()
    }
"#].concat(),
    );
}

#[test]
fn media_card_loads_with_the_native_backend_bridge_contract() {
    run_fixture(
        "media_card_loads",
        &[
            "PlayerCard.qml",
            "MediaCredit.qml",
            "Artwork.qml",
            "SeekBar.qml",
            "VolumeSlider.qml",
            "SourcesList.qml",
            "OutputList.qml",
            "shared/ControlButton.qml",
            "shared/IconButton.qml",
            "shared/PopupFrame.qml",
            "shared/StyledSlider.qml",
        ],
        "",
        r#"
    QtObject {
        id: fakePanel
        property string mediaControl: root.helper
        property string openScreen: ""
        property var activeGroup: null
        property var streamNode: null
        property var streamSink: null
        property var routes: []
        property var groups: []
        property bool canRoute: false
        property string routingStatus: ""
        property string routingError: ""
        function mediaAction(kind, player, position) { root.fail("unexpected control") }
        function selectGroup(group) { root.fail("unexpected selection") }
        function moveStream(key) { root.fail("unexpected routing") }
    }
    PlayerCard { id: card; width: 460; panel: fakePanel }
    function advance() {
        if (++phase >= 4) {
            check(card.player === null, "empty UI should have no native player")
            check(card.implicitHeight > 0, "media card failed to create its layout")
            pass()
        }
    }
"#,
    );
}

#[test]
fn media_credit_local_file_and_inactive_identity_roundtrips() {
    run_fixture(
        "credit_roundtrips",
        &["MediaCredit.qml"],
        "",
        CREDIT_ROUNDTRIPS,
    );
}
