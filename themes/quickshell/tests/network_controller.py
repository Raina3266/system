"""Test native-network orchestration with fake devices/helper, never real NM mutations.

Run: python3 themes/quickshell/tests/network_controller.py
Requires quickshell, busctl, and dbus-run-session on PATH.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


def main():
    if sys.argv[1:] != ["--isolated"]:
        os.execvp("dbus-run-session", ["dbus-run-session", "--", sys.executable, __file__, "--isolated"])
    source = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="network-controller-", dir=os.environ.get("DELTA_SCRATCH_DIR")) as directory:
        root = Path(directory)
        for filename in ("NetworkController.qml", "NetworkFormat.js"):
            shutil.copyfile(source / filename, root / filename)
        helper = root / "helper"
        helper.write_text(f"""#!{sys.executable}
import json, sys, time
from pathlib import Path
mode, interface, ssid = sys.argv[1:4]
identity = {{'interface': interface, 'ssid': ssid, 'profileUuid': 'test-uuid', 'bssid': '00:11:22:33:44:55'}}
if mode in ('secret', 'qr'):
    assert sys.argv[4:] == [identity['profileUuid'], identity['bssid']]
else:
    assert len(sys.argv) == 4
request = {{'mode': mode, 'interface': interface, 'ssid': ssid}}
if mode == 'connect-wep':
    request['stdinValid'] = json.load(sys.stdin) == {{'password': 'test-wep-key'}}
with Path({str(root / 'requests.jsonl')!r}).open('a') as log:
    log.write(json.dumps(request) + '\\n')
time.sleep(0.12)
if Path({str(root / 'mismatch')!r}).exists():
    identity['bssid'] = 'wrong'
if mode == 'info':
    print(json.dumps(dict(identity, profileId='Test profile', ipv4='192.0.2.2/24')))
elif mode == 'secret':
    print(json.dumps(dict(identity, password='TEST-SECRET')))
elif mode == 'qr':
    print(json.dumps(dict(identity, image='data:image/png;base64,TEST')))
else:
    print(json.dumps({{'success': True}}))
""")
        helper.chmod(0o700)
        (root / "shell.qml").write_text("""
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Networking
ShellRoot {
    component FakeNetwork: QtObject {
        property string name: ""
        property bool connected: false
        property bool stateChanging: false
        property bool known: true
        property real signalStrength: 0.8
        property int security: WifiSecurityType.Wpa2Psk
        property var device: wifi
        property var nmSettings: [{ id: "Home", uuid: "one" }, { id: "Home", uuid: "two" }]
        property int connections: 0
        property int forgets: 0
        property string profile: ""
        property bool passwordMatched: false
        signal connectionFailed(int reason)
        function connect() { connections++ }
        function connectWithSettings(settings) { connections++; profile = settings.uuid }
        function connectWithPsk(password) { connections++; passwordMatched = password === "test-password" }
        function forget() { forgets++ }
    }
    FakeNetwork { id: home; name: "Home" }
    FakeNetwork { id: guest; name: "Guest"; known: false; nmSettings: [] }
    FakeNetwork { id: wep; name: "Legacy"; known: false; nmSettings: []; security: WifiSecurityType.StaticWep }
    FakeNetwork { id: wired; name: "Wired"; connected: true; device: ethernet }
    QtObject {
        id: wifi
        property string name: "wlan-test"
        property int type: DeviceType.Wifi
        property bool scannerEnabled: false
        property int disconnects: 0
        property var networks: ({ values: [home, guest, wep] })
        function disconnect() { disconnects++; home.connected = false }
    }
    QtObject {
        id: ethernet
        property string name: "eth-test"
        property int type: DeviceType.Wired
        property int disconnects: 0
        property var networks: ({ values: [wired] })
        function disconnect() { disconnects++; wired.connected = false }
    }
    QtObject {
        id: fake
        property bool wifiEnabled: true
        property bool wifiHardwareEnabled: true
        property var devices: ({ values: [wifi, ethernet] })
    }
    NetworkController { id: center; backend: fake }
    property int resets: 0
    Connections {
        target: center.listModel
        function onModelReset() { resets++ }
    }
    function key(name) { return center.rows.find(row => row.name === name).key }
    IpcHandler {
        target: "test"
        function status(): string {
            return JSON.stringify({
                rows: center.rows.map(row => ({ name: row.name, key: row.key, strength: row.strength })),
                opened: center.opened, scanner: wifi.scannerEnabled,
                wifiEnabled: fake.wifiEnabled, error: center.error,
                credentialKey: center.credentialKey, page: center.page,
                details: center.details, detailsLoading: center.detailsLoading,
                secret: center.secret, qrImage: center.qrImage,
                homeConnections: home.connections, homeProfile: home.profile,
                guestConnections: guest.connections, passwordMatched: guest.passwordMatched,
                forgets: home.forgets, disconnects: wifi.disconnects,
                wepPasswordLength: center.wepPassword.length,
                pendingKey: center.pendingKey, resets: resets,
                secretLoading: center.secretLoading, qrLoading: center.qrLoading
            })
        }
        function open(): void { center.toggle("test-monitor") }
        function close(): void { center.close() }
        function connect(name: string, uuid: string): void { center.connectNetwork(key(name), uuid) }
        function password(value: string): void { center.submitPassword(value) }
        function missing(): void { home.connectionFailed(ConnectionFailReason.NoSecrets) }
        function guestFailed(): void { guest.connectionFailed(ConnectionFailReason.NoSecrets) }
        function terminal(): void { home.stateChanging = true; home.stateChanging = false }
        function signalUpdate(): void { home.signalStrength = 0.81 }
        function revealHide(): void { center.revealSecret(); center.hideSecret() }
        function monitor(): void { center.monitor = "other-monitor" }
        function reset(): void { center.back() }
        function singleProfile(): void { home.nmSettings = [{ id: "Home", uuid: "one" }] }
        function info(): void { center.info(key("Home")) }
        function qr(): void { center.qr(key("Home")) }
        function reveal(): void { center.revealSecret() }
        function hide(): void { center.hideSecret() }
        function connected(value: bool): void { home.connected = value }
        function forget(): void { center.requestForget(key("Home")) }
        function confirmForget(): void { center.confirmForget() }
        function toggleWifi(): void { center.toggleWifi() }
        function hardware(value: bool): void { fake.wifiHardwareEnabled = value }
        function badge(): string { return center.status() }
    }
}
""")
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QS_NO_RELOAD_POPUP="1", NETWORK_HELPER=str(helper))
        log = (root / "shell.log").open("w")
        process = subprocess.Popen(["quickshell", "--path", str(root)], env=env, stdout=log, stderr=log)

        def ipc(method, *args):
            return subprocess.check_output(["quickshell", "ipc", "--path", str(root), "call", "test", method, *args],
                                           env=env, text=True, stderr=subprocess.DEVNULL).strip()

        def status():
            return json.loads(ipc("status"))

        def wait_for(predicate, message):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    state = status()
                    if predicate(state):
                        return state
                except (subprocess.CalledProcessError, json.JSONDecodeError):
                    pass
                if process.poll() is not None:
                    break
                time.sleep(0.03)
            raise AssertionError(message)

        def requests():
            path = root / "requests.jsonl"
            return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

        try:
            state = wait_for(lambda state: len(state["rows"]) == 4, "fake backend did not load")
            assert not state["scanner"] and state["rows"][1]["strength"] == 80
            ipc("open")
            wait_for(lambda state: state["opened"] and state["scanner"], "panel did not enable native scanner")
            assert json.loads(ipc("badge"))["class"] == "ethernet"
            ipc("connect", "Home", "two")
            state = wait_for(lambda state: state["homeConnections"] == 1, "saved profile was not activated")
            assert state["homeProfile"] == "two"
            ipc("connect", "Home", "one")
            assert status()["homeConnections"] == 1
            ipc("missing")
            state = wait_for(lambda state: "Advanced" in state["error"], "per-profile credential fallback missing")
            assert not state["credentialKey"]
            ipc("connect", "Home", "")
            wait_for(lambda state: state["homeConnections"] == 2, "automatic connect missing")
            ipc("terminal")
            wait_for(lambda state: not state["pendingKey"], "terminal failure did not release activation")
            ipc("singleProfile")
            ipc("connect", "Home", "one")
            ipc("missing")
            wait_for(lambda state: bool(state["credentialKey"]), "missing secret did not prompt")
            ipc("connect", "Guest", "")
            state = wait_for(lambda state: bool(state["credentialKey"]), "new personal network did not prompt")
            assert state["guestConnections"] == 0
            ipc("password", "test-password")
            wait_for(lambda state: state["passwordMatched"] and not state["credentialKey"], "native PSK call failed")
            print("PASS native scanner, saved profile selection, and WPA credential flow", flush=True)

            ipc("guestFailed")
            ipc("connect", "Legacy", "")
            ipc("password", "test-wep-key")
            wait_for(lambda state: state["wepPasswordLength"] == 0, "WEP password retained in controller")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not any(row["mode"] == "connect-wep" for row in requests()):
                time.sleep(0.03)
            assert next(row for row in requests() if row["mode"] == "connect-wep")["stdinValid"]
            print("PASS WEP helper uses stdin and clears the submitted key", flush=True)

            ipc("connected", "true")
            ipc("info")
            wait_for(lambda state: state["details"].get("profileId") == "Test profile", "Info response missing")
            assert all(row["mode"] != "secret" for row in requests())
            before = len(requests())
            ipc("revealHide")
            time.sleep(0.2)
            assert len(requests()) == before, "hidden queued secret invoked helper"
            resets = status()["resets"]
            ipc("signalUpdate")
            assert status()["resets"] == resets
            (root / "mismatch").touch()
            ipc("reveal")
            wait_for(lambda state: not state["secretLoading"] and bool(state["error"]), "mismatched response accepted")
            assert not status()["secret"]
            (root / "mismatch").unlink()
            ipc("reveal")
            wait_for(lambda state: state["secret"] == "TEST-SECRET", "explicit reveal failed")
            ipc("hide")
            assert status()["secret"] == ""
            ipc("qr")
            wait_for(lambda state: bool(state["qrImage"]), "explicit QR response missing")
            ipc("close")
            state = wait_for(lambda state: not state["opened"] and not state["scanner"], "close did not stop scanner")
            assert state["secret"] == state["qrImage"] == "" and state["details"] == {}
            print("PASS explicit-only secrets/QR and dismissal cleanup", flush=True)

            ipc("open")
            helper.rename(root / "helper-disabled")
            ipc("info")
            wait_for(lambda state: not state["detailsLoading"] and bool(state["error"]), "failed start retained Loading")
            (root / "helper-disabled").rename(helper)
            ipc("info")
            ipc("monitor")
            time.sleep(0.2)
            assert status()["page"] == "list" and status()["details"] == {}
            ipc("info")
            ipc("connected", "false")
            time.sleep(0.25)
            state = status()
            assert state["page"] == "list" and state["details"] == {} and state["secret"] == ""
            ipc("connected", "true")
            ipc("forget")
            assert status()["forgets"] == 0
            ipc("confirmForget")
            state = wait_for(lambda state: state["forgets"] == 1, "confirmed forget missing")
            assert state["disconnects"] == 1
            ipc("hardware", "false")
            assert status()["page"] == "list" and not status()["pendingKey"]
            ipc("connect", "Home", "")
            count = status()["homeConnections"]
            time.sleep(0.1)
            assert status()["homeConnections"] == count
            ipc("toggleWifi")
            assert status()["wifiEnabled"]
            ipc("hardware", "true")
            ipc("connected", "true")
            ipc("info")
            wait_for(lambda state: not state["detailsLoading"] and bool(state["details"]), "details did not reopen")
            ipc("reveal")
            ipc("reset")
            time.sleep(0.2)
            state = status()
            assert state["details"] == {} and not state["secret"] and not state["secretLoading"]
            ipc("info")
            ipc("toggleWifi")
            time.sleep(0.2)
            state = status()
            assert not state["wifiEnabled"] and state["page"] == "list" and state["details"] == {}
            print("PASS stale result rejection, confirmed forget/disconnect, and hardware-block guard", flush=True)
        except Exception:
            log.flush()
            print((root / "shell.log").read_text(), file=sys.stderr)
            raise
        finally:
            process.terminate()
            process.wait(timeout=5)
            log.close()


if __name__ == "__main__":
    main()
