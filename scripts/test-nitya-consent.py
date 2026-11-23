"""Tests nitya-consent against a fake provider on loopback, for both providers.

    python3 scripts/test-nitya-consent.py <nitya-consent command...>

The fake checks what a real provider would: the PKCE challenge against the verifier, the
code, the redirect URI and the client; the test plays the browser, following the sign-in
URL to the loopback redirect. Also checks that a wrong state is refused. Offline; run by
`nix flake check` as checks.nitya-consent.
"""

import base64
import hashlib
import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import urllib.parse
import urllib.request

cmd = sys.argv[1:]
seen = {}


class Fake(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = urllib.parse.parse_qs(self.rfile.read(int(self.headers["Content-Length"])).decode())
        f = {k: v[0] for k, v in body.items()}
        challenge = base64.urlsafe_b64encode(hashlib.sha256(f["code_verifier"].encode()).digest()).rstrip(b"=").decode()
        ok = (
            f["grant_type"] == "authorization_code"
            and f["code"] == "the-code"
            and challenge == seen["code_challenge"]
            and f["redirect_uri"] == seen["redirect_uri"]
            and f["client_id"] == seen["client_id"]
            and (seen["provider"] != "google" or f.get("client_secret") == "s3cret")
        )
        self.send_response(200 if ok else 400)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"refresh_token": "the-refresh-token", "scope": "a b"} if ok else {"error": "invalid_grant"}).encode())

    def log_message(self, *a):
        pass


fake = http.server.HTTPServer(("127.0.0.1", 0), Fake)
threading.Thread(target=fake.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{fake.server_address[1]}"


def run(provider, args, tamper_state=False):
    out = tempfile.mktemp()
    p = subprocess.Popen(
        [*cmd, provider, *args, "--no-browser", "--timeout", "20",
         "--auth-url", f"{base}/auth", "--token-url", f"{base}/token", "--out", out],
        stderr=subprocess.PIPE, text=True,
    )
    url = None
    for line in p.stderr:
        if line.strip().startswith(f"{base}/auth?"):
            url = line.strip()
            break
    q = {k: v[0] for k, v in urllib.parse.parse_qs(urllib.parse.urlsplit(url).query).items()}
    seen.update(q, provider=provider)
    assert q["code_challenge_method"] == "S256" and q["response_type"] == "code"
    if provider == "google":
        assert q["access_type"] == "offline" and q["prompt"] == "consent", q
    # the browser: the provider redirects to the loopback address with the code
    redirect = q["redirect_uri"].replace("//localhost:", "//127.0.0.1:")
    state = "not-the-state" if tamper_state else q["state"]
    urllib.request.urlopen(f"{redirect}?code=the-code&state={state}").read()
    err = p.stderr.read()
    p.wait()
    return p.returncode, out, err


client = tempfile.mktemp(suffix=".json")
with open(client, "w") as f:
    json.dump({"installed": {"client_id": "g-client", "client_secret": "s3cret"}}, f)

for provider, args, cid in [("google", ["--client-json", client], "g-client"),
                            ("microsoft", ["--client-id", "m-client"], "m-client")]:
    code, out, err = run(provider, args)
    assert code == 0, err
    data = json.load(open(out))
    assert data["provider"] == provider and data["client_id"] == cid, data
    assert data["refresh_token"] == "the-refresh-token", data
    assert oct(os.stat(out).st_mode & 0o777) == "0o600", oct(os.stat(out).st_mode)
    host = "127.0.0.1" if provider == "google" else "localhost"
    assert seen["redirect_uri"].startswith(f"http://{host}:"), seen["redirect_uri"]
    print(f"ok {provider}")

code, out, err = run("microsoft", ["--client-id", "m-client"], tamper_state=True)
assert code != 0 and "state" in err and not os.path.exists(out), (code, err)
print("ok wrong state refused")
