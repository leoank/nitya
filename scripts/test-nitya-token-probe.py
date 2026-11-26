"""Tests nitya-token-probe against a fake of every endpoint it calls, offline.

    python3 scripts/test-nitya-token-probe.py <nitya-token-probe command...>

The fake rotates Microsoft refresh tokens as Microsoft does and accepts every token it
issued; it checks the Google client secret and the iCloud password. Two checks in a row
must keep the rotated token in the state directory and use it; a revoked Google token
must be logged as invalid_grant with exit status 1; no token may appear in the log.
Run by `nix flake check` as checks.nitya-token-probe.
"""

import base64
import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import urllib.parse

cmd = sys.argv[1:]
issued = {"m-original"}
state = {"google_revoked": False, "ms_refreshes": []}


class Fake(http.server.BaseHTTPRequestHandler):
    def reply(self, code, obj):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(obj).encode())

    def do_POST(self):
        f = {k: v[0] for k, v in urllib.parse.parse_qs(self.rfile.read(int(self.headers["Content-Length"])).decode()).items()}
        if self.path == "/gtoken":
            ok = f["refresh_token"] == "g-refresh" and f["client_secret"] == "g-secret" and not state["google_revoked"]
            return self.reply(200, {"access_token": "g-access"}) if ok else self.reply(400, {"error": "invalid_grant"})
        if self.path == "/mtoken":
            state["ms_refreshes"].append(f["refresh_token"])
            if f["refresh_token"] not in issued or f["client_id"] != "m-client":
                return self.reply(400, {"error": "invalid_grant"})
            new = f"m-rotated-{len(issued)}"
            issued.add(new)
            return self.reply(200, {"access_token": "m-access", "refresh_token": new})
        self.reply(404, {})

    def do_GET(self):
        auth = self.headers.get("Authorization", "")
        data = {"/gcals": ("Bearer g-access", {"items": [{"id": "a"}, {"id": "b"}]}),
                "/gmail": ("Bearer g-access", {"messages": [{"id": "msg-1"}]}),
                "/mcals": ("Bearer m-access", {"value": [{"id": "c"}]}),
                "/mmail": ("Bearer m-access", {"value": [{"id": "msg-2"}]})}
        want, body = data.get(self.path.split("?")[0], (None, None))
        self.reply(200, body) if want and auth == want else self.reply(401, {"error": {"code": "InvalidAuthenticationToken"}})

    def do_PROPFIND(self):
        ok = self.headers.get("Authorization") == "Basic " + base64.b64encode(b"me@example.org:abcd-efgh-ijkl-mnop").decode()
        self.send_response(207 if ok else 401)
        self.send_header("Content-Type", "application/xml")
        self.end_headers()
        self.wfile.write(b"<multistatus xmlns='DAV:'/>")

    def log_message(self, *a):
        pass


fake = http.server.HTTPServer(("127.0.0.1", 0), Fake)
threading.Thread(target=fake.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{fake.server_address[1]}"
env = {**os.environ, "NITYA_PROBE_ENDPOINTS": json.dumps({
    "google_token": f"{base}/gtoken", "google_calendars": f"{base}/gcals", "gmail_latest": f"{base}/gmail",
    "microsoft_token": f"{base}/mtoken", "graph_calendars": f"{base}/mcals", "graph_latest": f"{base}/mmail",
    "icloud": f"{base}/"})}

d = tempfile.mkdtemp()
files = {"gclient.json": {"installed": {"client_id": "g-client", "client_secret": "g-secret"}},
         "gtoken.json": {"provider": "google", "refresh_token": "g-refresh"},
         "mtoken.json": {"provider": "microsoft", "client_id": "m-client", "token_uri": "unused", "refresh_token": "m-original"},
         "icloud.json": {"username": "me@example.org", "app_password": "abcd-efgh-ijkl-mnop"}}
for name, obj in files.items():
    with open(os.path.join(d, name), "w") as f:
        json.dump(obj, f)
st = os.path.join(d, "state")
args = ["check", "--state", st, "--google-client", f"{d}/gclient.json", "--google-token", f"{d}/gtoken.json",
        "--microsoft-token", f"{d}/mtoken.json", "--icloud", f"{d}/icloud.json"]


def check():
    p = subprocess.run([*cmd, *args], env=env, capture_output=True, text=True)
    return p.returncode, [json.loads(l) for l in p.stdout.splitlines()]


code, lines = check()
assert code == 0, lines
assert [(x["provider"], x["chain"], x["ok"]) for x in lines] == [
    ("google", "token", True), ("microsoft", "latest", True), ("microsoft", "original", True), ("icloud", "app-password", True)], lines
assert lines[0]["calendars"] == 2 and lines[1]["rotated"] is True, lines
latest = json.load(open(os.path.join(st, "microsoft-latest.json")))["refresh_token"]
assert latest.startswith("m-rotated-") and oct(os.stat(os.path.join(st, "microsoft-latest.json")).st_mode & 0o777) == "0o600"
print("ok first check, Microsoft's rotated token kept")

state["ms_refreshes"].clear()
state["google_revoked"] = True
code, lines = check()
assert state["ms_refreshes"] == [latest, "m-original"], state["ms_refreshes"]  # the latest chain used the kept token
g = lines[0]
assert code == 1 and not g["ok"] and g["step"] == "refresh" and g["status"] == 400 and g["error"] == "invalid_grant", lines
print("ok second check used the kept token; a revoked Google token is logged as invalid_grant, exit 1")

log = open(os.path.join(st, "probe.jsonl")).read()
for secret in ("g-refresh", "g-secret", "m-original", "m-rotated", "abcd-efgh", "g-access", "m-access", "msg-1", "msg-2"):
    assert secret not in log, secret
print("ok no token or message id in the log")

rep = subprocess.run([*cmd, "report", os.path.join(st, "probe.jsonl")], capture_output=True, text=True, check=True).stdout
result = json.loads(rep.split("===RESULT=== ")[1])
assert result["chains"] == 4 and result["all_ok"] is False and result["google_token_failures"] == 1, rep
print("ok report")
