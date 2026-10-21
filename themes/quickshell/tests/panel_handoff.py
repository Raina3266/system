"""Test the real shell's AGS handoff with fake panels on a private D-Bus.

Run: python3 themes/quickshell/tests/panel_handoff.py
Requires quickshell and dbus-run-session; never controls native media/network.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main():
    if sys.argv[1:] != ["--isolated"]:
        os.execvp("dbus-run-session", ["dbus-run-session", "--", sys.executable, __file__, "--isolated"])
    source = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="panel-handoff-", dir=os.environ.get("DELTA_SCRATCH_DIR")) as directory:
        root = Path(directory)
        shutil.copyfile(source / "shell.qml", root / "shell.qml")
        (root / "NetworkController.qml").write_text("""
import Quickshell
Scope {
    property bool opened: false
    property string monitor: ""
    function toggle(name) { opened = !(opened && monitor === name); monitor = name }
    function close() { opened = false }
    function status() { return JSON.stringify({opened: opened, monitor: monitor}) }
}
""")
        (root / "Notifications.qml").write_text("""
import Quickshell
Scope {
    property var history: []
    property var popups: []
    property bool dnd: false
    function toggleDnd() { dnd = !dnd }
    function clearNotifications() {}
    function dismissNotification(id) {}
    function invokeNotificationAction(id, identifier) {}
}
""")
        for name, properties in {
            "NotificationPopups": "property var center",
            "NetworkPanel": "property var controller",
            "DashboardPanel": "property var panel; property var modelData",
            "IdleInhibitorWindow": "property var panel",
        }.items():
            (root / f"{name}.qml").write_text(f"import Quickshell\nScope {{ {properties} }}\n")
        calls = root / "calls.jsonl"
        helper = root / "media-panel"
        helper.write_text(f"""#!{sys.executable}
import json, sys
with open({str(calls)!r}, 'a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\\n')
""")
        helper.chmod(0o700)
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QS_NO_RELOAD_POPUP="1", MEDIA_PANEL=str(helper))
        with (root / "shell.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root)], env=env, stdout=log, stderr=log)

            def ipc(target, method, *args):
                return subprocess.check_output(
                    ["quickshell", "ipc", "--path", str(root), "call", target, method, *args],
                    env=env, text=True, stderr=subprocess.DEVNULL, timeout=5,
                ).strip()

            def requests():
                return [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []

            def wait_for(predicate, message):
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        break
                    try:
                        if predicate():
                            return
                    except (subprocess.SubprocessError, json.JSONDecodeError):
                        pass
                    time.sleep(0.05)
                raise AssertionError(message)

            try:
                wait_for(lambda: json.loads(ipc("network", "status"))["opened"] is False, "shell did not start")
                for _ in range(3):
                    ipc("network", "status")
                    ipc("notifications", "status")
                assert requests() == [], "status started a media subprocess"
                ipc("network", "toggle", "test-monitor")
                wait_for(lambda: requests() == [["close"]], "opening network did not close AGS media")
                for _ in range(5):
                    ipc("network", "status")
                ipc("network", "toggle", "test-monitor")
                assert json.loads(ipc("network", "status"))["opened"] is False
                time.sleep(0.2)
                assert requests() == [["close"]], "status or closing network started another media subprocess"
                ipc("network", "toggle", "other-monitor")
                wait_for(lambda: requests() == [["close"], ["close"]], "reopening network did not close media")
                ipc("network", "close")
                ipc("dashboard", "toggle", "test-monitor")
                ipc("notifications", "toggleDnd")
                assert json.loads(ipc("notifications", "status"))["dnd"] is True
                assert requests() == [["close"], ["close"]]
                print("PASS network-open AGS handoff; status, close, dashboard, and notification IPC stay independent")
            except Exception:
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()
