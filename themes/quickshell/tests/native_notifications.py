"""Exercise the real Quickshell notification protocol on an isolated session bus.

Run: python3 themes/quickshell/tests/native_notifications.py
Requires quickshell, busctl, and dbus-run-session on PATH. Never uses the
desktop's notification bus, history file, or popup windows.
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
    with tempfile.TemporaryDirectory(prefix="quickshell-notifications-", dir=os.environ.get("DELTA_SCRATCH_DIR")) as directory:
        root = Path(directory)
        for filename in ("Notifications.qml", "NotificationStore.js"):
            shutil.copyfile(source / filename, root / filename)
        (root / "shell.qml").write_text("""
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    id: shell
    property int generation: 1
    Notifications { id: center }
    IpcHandler {
        target: "test"
        function status(): string {
            return JSON.stringify({ history: center.history, popups: center.popups, dnd: center.dnd, generation: shell.generation })
        }
        function toggleDnd(): void { center.toggleDnd() }
        function clear(): void { center.clearNotifications() }
        function hover(id: string, hovered: bool): void { center.setHovered(id, hovered) }
        function invoke(id: string, action: string): void { center.invokeNotificationAction(id, action) }
    }
}
""")
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QS_NO_RELOAD_POPUP="1",
                   NOTIFICATION_HISTORY=str(root / "history.json"))
        log = (root / "shell.log").open("w+")
        process = None

        def ipc(method, *arguments):
            return subprocess.check_output(
                ["quickshell", "ipc", "--path", str(root), "call", "test", method, *arguments],
                env=env, text=True, stderr=subprocess.DEVNULL
            ).strip()

        def status():
            return json.loads(ipc("status"))

        def wait_for(predicate, description, timeout=5):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                try:
                    value = status()
                    if predicate(value):
                        return value
                except (subprocess.CalledProcessError, json.JSONDecodeError):
                    pass
                if process.poll() is not None:
                    break
                time.sleep(0.03)
            raise AssertionError(description)

        def call(method, signature="", *arguments):
            command = ["busctl", "--user", "--json=short", "call", "org.freedesktop.Notifications",
                       "/org/freedesktop/Notifications", "org.freedesktop.Notifications", method]
            if signature:
                command.extend([signature, *arguments])
            output = subprocess.check_output(command, text=True)
            return json.loads(output)["data"] if output.strip() else None

        def notify(summary, timeout=0, replaces=0, actions=(), hints=()):
            arguments = ["Native test", str(replaces), "", summary, "Test body", str(len(actions)), *actions,
                         str(len(hints))]
            for name, kind, value in hints:
                arguments.extend([name, kind, value])
            arguments.append(str(timeout))
            return call("Notify", "susssasa{sv}i", *arguments)[0]

        def start():
            return subprocess.Popen(["quickshell", "--path", str(root)], env=env, stdout=log, stderr=log)

        try:
            process = start()
            wait_for(lambda state: state["history"] == [], "notification server did not start")
            assert call("GetServerInformation")[0] == "quickshell"

            first = notify("Original")
            state = wait_for(lambda state: len(state["popups"]) == 1, "notification was not received")
            key = state["history"][0]["id"]
            assert not key.startswith("undefined:")
            assert notify("Replacement", replaces=first) == first
            state = wait_for(lambda state: state["history"][0]["summary"] == "Replacement", "replacement was not updated")
            assert len(state["history"]) == 1 and len(state["popups"]) == 1
            call("CloseNotification", "u", str(first))
            wait_for(lambda state: not state["history"] and not state["popups"], "client close did not remove notification")
            print("PASS receive, replacement, and client close", flush=True)

            notify("Expiring", timeout=350)
            state = wait_for(lambda state: len(state["history"]) == 1, "expiring notification was not received")
            assert state["history"][0]["timeout"] == 350
            wait_for(lambda state: state["history"][0]["closed"] and not state["popups"], "expiry did not archive history")
            print("PASS native timeout units and archived history", flush=True)

            ipc("toggleDnd")
            notify("Quiet")
            state = wait_for(lambda state: state["history"][0]["summary"] == "Quiet", "DND notification was not saved")
            assert state["dnd"] and not state["popups"]
            ipc("toggleDnd")
            transient = notify("Transient", hints=[("transient", "b", "true")])
            state = wait_for(lambda state: len(state["popups"]) == 1, "transient popup missing")
            assert all(row["summary"] != "Transient" for row in state["history"])
            call("CloseNotification", "u", str(transient))
            print("PASS DND and transient notifications", flush=True)

            notify("Hover", timeout=500)
            state = wait_for(lambda state: state["popups"] and state["popups"][0]["summary"] == "Hover", "hover popup missing")
            hover_key = state["popups"][0]["id"]
            ipc("hover", hover_key, "true")
            time.sleep(0.7)
            assert any(row["id"] == hover_key for row in status()["popups"])
            ipc("hover", hover_key, "false")
            wait_for(lambda state: not state["popups"], "hover leave did not resume expiry")
            notify("Critical", timeout=300, hints=[("urgency", "y", "2")])
            wait_for(lambda state: bool(state["popups"]), "critical popup missing")
            time.sleep(0.5)
            assert status()["popups"][0]["summary"] == "Critical"
            print("PASS hover pause and critical persistence", flush=True)

            action_id = notify("Action", actions=["open", "Open"])
            state = wait_for(lambda state: state["popups"][0]["summary"] == "Action", "action popup missing")
            action_key = state["popups"][0]["id"]
            assert state["popups"][0]["actions"] == [{"identifier": "open", "text": "Open"}]
            assert notify("Action updated", replaces=action_id, actions=["open", "Updated"]) == action_id
            wait_for(lambda state: state["popups"][0]["actions"] == [{"identifier": "open", "text": "Updated"}],
                     "native replacement action label did not update")
            ipc("invoke", action_key, "open")
            wait_for(lambda state: all(row["id"] != action_key for row in state["history"]), "native action did not dismiss notification")
            for index in range(7):
                notify(f"Stack {index}")
            state = wait_for(lambda state: len(state["popups"]) == 5 and state["popups"][0]["summary"] == "Stack 6", "popup stack limit or ordering is wrong")
            assert len({row["id"] for row in state["popups"]}) == 5
            print("PASS native actions and bounded newest-first stack", flush=True)

            notify("Reload action", actions=["open", "Open"])
            state = wait_for(lambda state: state["history"][0]["summary"] == "Reload action", "reload fixture missing")
            reload_key = state["history"][0]["id"]
            replaced = notify("Before replacement")
            notify("Second newest")
            notify("Newest replacement", replaces=replaced)
            state = wait_for(lambda state: state["history"][0]["summary"] == "Newest replacement", "reload ordering fixture missing")
            expected_order = [row["id"] for row in state["history"]]
            expected_count = len(state["history"])
            config = root / "shell.qml"
            config.write_text(config.read_text().replace("property int generation: 1", "property int generation: 2"))
            state = wait_for(lambda state: state["generation"] == 2 and len(state["history"]) == expected_count, "QML reload duplicated or lost history")
            assert not state["popups"]
            assert [row["id"] for row in state["history"]] == expected_order
            row = next(row for row in state["history"] if row["id"] == reload_key)
            assert not row["closed"] and row["actions"] == [{"identifier": "open", "text": "Open"}]
            ipc("invoke", reload_key, "open")
            wait_for(lambda state: all(row["id"] != reload_key for row in state["history"]), "action did not survive QML reload")
            print("PASS QML reload preserves history and native actions without replaying popups", flush=True)

            ipc("toggleDnd")
            config.write_text(config.read_text().replace("property int generation: 2", "property int generation: 3"))
            wait_for(lambda state: state["generation"] == 3 and state["dnd"], "immediate reload lost DND state")
            ipc("clear")
            config.write_text(config.read_text().replace("property int generation: 3", "property int generation: 4"))
            wait_for(lambda state: state["generation"] == 4 and not state["history"], "immediate reload resurrected cleared history")
            notify("Persisted while quiet")
            wait_for(lambda state: state["dnd"] and not state["popups"], "DND did not hide existing popups")
            wait_for(lambda state: len(state["history"]) == 1, "quiet persistence fixture missing")
            expected_count = len(status()["history"])
            process.terminate()
            process.wait(timeout=5)
            process = start()
            state = wait_for(lambda state: state["dnd"] and len(state["history"]) == expected_count, "history/DND did not survive a process restart")
            assert all(row["closed"] and row["actions"] == [] for row in state["history"])
            ipc("clear")
            wait_for(lambda state: not state["history"] and not state["popups"], "clear all failed")
            print("PASS history/DND persistence and clear all", flush=True)

            ipc("toggleDnd")
            oldest = notify("Bounded 0")
            for index in range(1, 201):
                notify(f"Bounded {index}")
            wait_for(lambda state: len(state["history"]) == 200 and state["history"][0]["summary"] == "Bounded 200", "history limit is wrong")
            assert notify("Evicted replacement", replaces=oldest) != oldest
            ipc("clear")
            oldest = notify("Transient 0", hints=[("transient", "b", "true")])
            for index in range(1, 6):
                notify(f"Transient {index}", hints=[("transient", "b", "true")])
            wait_for(lambda state: len(state["popups"]) == 5 and not state["history"], "transient stack limit is wrong")
            assert notify("Evicted transient", replaces=oldest, hints=[("transient", "b", "true")]) != oldest
            ipc("toggleDnd")
            wait_for(lambda state: state["dnd"] and not state["popups"], "DND did not hide transients")
            transient = notify("Quiet transient", hints=[("transient", "b", "true")])
            time.sleep(0.1)
            assert notify("Quiet replacement", replaces=transient, hints=[("transient", "b", "true")]) != transient
            print("PASS evicted and DND-suppressed native objects are released", flush=True)
        except Exception:
            log.flush()
            print((root / "shell.log").read_text(), file=sys.stderr)
            if process.poll() is None:
                try:
                    print("Last test state:", status(), file=sys.stderr)
                except (subprocess.CalledProcessError, json.JSONDecodeError):
                    pass
            raise
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            log.close()


if __name__ == "__main__":
    main()
