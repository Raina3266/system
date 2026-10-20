"""Credential-safe protocol tests: python3 test_protocol.py /path/to/network-manager."""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

BINARY = str(Path(sys.argv.pop()).resolve())
FAKE = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
root = pathlib.Path(os.environ["FIXTURE"])
with (root / "calls").open("a") as log:
    log.write(json.dumps(args) + "\n")
mode = os.environ.get("MODE", "")
if "--ask" in args:
    supplied = sys.stdin.read()
    if supplied != "DO_NOT_LEAK\n": sys.exit(9)
    print("DO_NOT_LEAK")
    print("DO_NOT_LEAK", file=sys.stderr)
    sys.exit(1 if mode == "wep-fail" else 0)
if "--show-secrets" in args:
    (root / "secret-read").touch()
    if mode == "failure":
        print("DO_NOT_LEAK", file=sys.stderr)
        sys.exit(1)
    print("DO_NOT_LEAK")
    sys.exit()
key = args[args.index("--get-values") + 1]
changed = (root / "secret-read").exists()
if key == "SSID,BSSID,SECURITY":
    print(r"Duplicate:11\:22\:33\:44\:55\:66:WPA2")
    if mode != "wep-absent":
        print(r"Other:22\:22\:33\:44\:55\:66:WEP")
        print(r"Duplicate:aa\:bb\:cc\:dd\:ee\:ff:WEP")
        print(r"Duplicate:00\:22\:33\:44\:55\:66:WEP")
elif key == "802-11-wireless.ssid": print("Duplicate")
elif key == "GENERAL.STATE": print("100 (connected)")
elif key == "GENERAL.CON-PATH": print("/active/1")
elif key == "GENERAL.HWADDR":
    if mode == "info-stale": (root / "secret-read").touch()
    print("aa")
elif key == "GENERAL.CON-UUID":
    switched = mode == "initial-profile" or (changed and mode in ("profile", "info-stale")) or (root / "qr-finished").exists()
    print("other-profile" if switched else "active-uuid")
elif key == "connection.type": print("802-3-ethernet" if mode == "wired" else "802-11-wireless")
elif key.startswith("IN-USE"):
    bssid = "dd" if mode == "initial-roam" or (changed and mode == "roam") else "aa"
    if mode.startswith("wep-"): bssid = r"aa\:bb\:cc\:dd\:ee\:ff"
    if mode == "wep-wrong": bssid = r"00\:22\:33\:44\:55\:66"
    ssid = "Other" if changed and mode == "ssid" else "Duplicate"
    print(f"*:{ssid}:{bssid}:6115 MHz:33:Infra:100 Mbit/s")
elif key == "802-11-wireless-security.key-mgmt":
    print("none" if mode.startswith("wep-") else {"open": "", "owe": "owe", "enterprise": "wpa-eap", "wep": "none"}.get(mode, "wpa-psk"))
elif key == "802-11-wireless-security.wep-tx-keyidx": print("2")
elif key == "802-11-wireless.hidden": print("yes")
elif key == "connection.id": print("Duplicate")
else: print("")
'''
QR = r'''#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path(os.environ["FIXTURE"])
(root / "qr-args").write_text(repr(sys.argv[1:]))
(root / "payload").write_bytes(sys.stdin.buffer.read())
if os.environ.get("MODE") == "qr-stale": (root / "qr-finished").touch()
sys.stdout.buffer.write(b"\x89PNG\r\n\x1a\nfake")
'''


class Protocol(unittest.TestCase):
    def invoke(self, command, mode="", ssid="Duplicate", input=None,
               uuid="active-uuid", bssid="aa", pin=True):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name, text in [("nmcli", FAKE), ("qrencode", QR)]:
            path = self.root / name
            path.write_text(text)
            path.chmod(0o700)
        args = [BINARY, command, "wlan0", ssid]
        if command in ("secret", "qr") and pin:
            args += [uuid, bssid]
        result = subprocess.run(
            args, capture_output=True, text=True, input=input,
            env={**os.environ, "FIXTURE": str(self.root), "MODE": mode,
                 "NETWORK_MANAGER_NMCLI": str(self.root / "nmcli"),
                 "NETWORK_MANAGER_QRENCODE": str(self.root / "qrencode")})
        self.assertNotIn("DO_NOT_LEAK", result.stderr)
        self.value = json.loads(result.stdout)
        self.calls = [json.loads(line) for line in (self.root / "calls").read_text().splitlines()] if (self.root / "calls").exists() else []
        return result

    def test_wep_stdin_and_pinned_cached_target(self):
        result = self.invoke("connect-wep", "wep-ok", input='{"password":"DO_NOT_LEAK"}')
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertTrue(self.value["success"])
        self.assertEqual(self.value["profileUuid"], "active-uuid")
        self.assertNotIn("DO_NOT_LEAK", result.stdout)
        self.assertNotIn("DO_NOT_LEAK", json.dumps(self.calls))
        activation = next(c for c in self.calls if "--ask" in c)
        self.assertEqual(activation, ["--ask", "--wait", "60", "device", "wifi",
            "connect", "aa:bb:cc:dd:ee:ff", "ifname", "wlan0",
            "bssid", "aa:bb:cc:dd:ee:ff"])
        cached = next(c for c in self.calls if "SSID,BSSID,SECURITY" in c)
        self.assertEqual(cached[-4:], ["ifname", "wlan0", "--rescan", "no"])
        self.assertFalse(any("--show-secrets" in c for c in self.calls))

    def test_wep_safe_failures_and_revalidation(self):
        for mode in ["wep-fail", "wep-wrong", "wep-absent"]:
            result = self.invoke("connect-wep", mode, input='{"password":"DO_NOT_LEAK"}')
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("DO_NOT_LEAK", result.stdout + result.stderr)
            if mode == "wep-absent":
                self.assertFalse(any("--ask" in c for c in self.calls))

    def test_wep_invalid_input_never_runs_nmcli(self):
        for payload in ["", "DO_NOT_LEAK", "{}", '{"password":12}',
                        '{"password":""}', '{"password":"DO_NOT_LEAK\\n"}',
                        '{"password":"DO_NOT_LEAK","extra":true}',
                        '{"password":"DO_NOT_LEAK"} {}', "x" * 4097]:
            result = self.invoke("connect-wep", input=payload)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.calls, [])
            self.assertNotIn("DO_NOT_LEAK", result.stdout + result.stderr)

    def test_info_never_requests_secrets(self):
        self.assertEqual(self.invoke("info").returncode, 0)
        self.assertEqual(len(self.value), 17)
        self.assertEqual(self.value["profileUuid"], "active-uuid")
        self.assertEqual(self.value["band"], "6 GHz")
        self.assertFalse(any("--show-secrets" in c for c in self.calls))
        self.assertNotIn("password", self.value)

    def test_secret_uses_active_uuid_not_duplicate_name(self):
        self.assertEqual(self.invoke("secret").returncode, 0)
        self.assertEqual(self.value, {"password": "DO_NOT_LEAK", "interface": "wlan0",
                                     "ssid": "Duplicate", "profileUuid": "active-uuid", "bssid": "aa"})
        secret = next(c for c in self.calls if "--show-secrets" in c)
        self.assertEqual(secret[-2:], ["uuid", "active-uuid"])

    def test_initial_same_ssid_switch_never_reads_or_encodes(self):
        for command in ("secret", "qr"):
            for mode in ("initial-profile", "initial-roam"):
                with self.subTest(command=command, mode=mode):
                    self.assertEqual(self.invoke(command, mode).returncode, 2)
                    self.assertEqual(set(self.value), {"error"})
                    self.assertFalse(any("--show-secrets" in c for c in self.calls))
                    self.assertFalse((self.root / "qr-args").exists())

    def test_missing_identity_never_runs_children(self):
        for command in ("secret", "qr"):
            for options in ({"pin": False}, {"uuid": ""}, {"bssid": ""}):
                with self.subTest(command=command, options=options):
                    self.assertEqual(self.invoke(command, **options).returncode, 2)
                    self.assertEqual(set(self.value), {"error"})
                    self.assertEqual(self.calls, [])
                    self.assertFalse((self.root / "qr-args").exists())

    def test_reject_stale_identity(self):
        for mode in ["profile", "roam", "ssid"]:
            with self.subTest(mode=mode):
                self.assertNotEqual(self.invoke("secret", mode).returncode, 0)
                self.assertNotIn("DO_NOT_LEAK", json.dumps(self.value))

    def test_failure_does_not_forward_secret_stderr(self):
        self.assertNotEqual(self.invoke("secret", "failure").returncode, 0)
        self.assertNotIn("DO_NOT_LEAK", json.dumps(self.value))

    def test_info_and_qr_final_revalidation(self):
        for command, mode in [("info", "info-stale"), ("qr", "qr-stale")]:
            self.assertNotEqual(self.invoke(command, mode).returncode, 0)
            self.assertEqual(set(self.value), {"error"})
            if command == "qr":
                self.assertTrue((self.root / "qr-finished").exists())

    def test_enterprise_and_owe_never_read_secrets(self):
        for mode in ["enterprise", "owe"]:
            for command in ["secret", "qr"]:
                self.assertNotEqual(self.invoke(command, mode).returncode, 0)
                self.assertFalse(any("--show-secrets" in c for c in self.calls))

    def test_empty_ssid_not_wireless_wildcard(self):
        self.assertNotEqual(self.invoke("info", ssid="").returncode, 0)

    def test_wired_info_and_secret_rejection(self):
        self.assertEqual(self.invoke("info", "wired", ssid="").returncode, 0)
        self.assertEqual(self.value["ssid"], "")
        self.assertEqual(self.value["security"], "wired")
        self.assertEqual(self.value["bssid"], "")
        self.assertNotEqual(self.invoke("secret", "wired", ssid="").returncode, 0)
        self.assertFalse(any("--show-secrets" in c for c in self.calls))

    def test_qr_stdin_only_and_wep_index(self):
        self.assertEqual(self.invoke("qr", "wep").returncode, 0)
        self.assertEqual(set(self.value), {"image", "interface", "ssid", "profileUuid", "bssid"})
        self.assertEqual({key: self.value[key] for key in ("interface", "ssid", "profileUuid", "bssid")},
                         {"interface": "wlan0", "ssid": "Duplicate", "profileUuid": "active-uuid", "bssid": "aa"})
        self.assertIn("data:image/png;base64,", self.value["image"])
        self.assertTrue(base64.b64decode(self.value["image"].split(",")[1]).startswith(b"\x89PNG"))
        self.assertIn("wep-key2", " ".join(next(c for c in self.calls if "--show-secrets" in c)))
        self.assertEqual((self.root / "payload").read_text(),
                         "WIFI:T:WEP;S:Duplicate;P:DO_NOT_LEAK;H:true;;")
        self.assertNotIn("DO_NOT_LEAK", (self.root / "qr-args").read_text())

    def test_hidden_open_qr(self):
        self.assertEqual(self.invoke("qr", "open").returncode, 0)
        self.assertEqual((self.root / "payload").read_text(), "WIFI:T:nopass;S:Duplicate;H:true;;")
        self.assertFalse(any("--show-secrets" in c for c in self.calls))


if __name__ == "__main__":
    unittest.main()
