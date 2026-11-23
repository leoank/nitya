"""nitya-consent: sign in to Google or Microsoft once and get a refresh token for nitya.

Runs the OAuth authorization-code flow of a desktop app with PKCE and a loopback redirect
(RFC 8252): opens the provider's consent page in a browser, receives the code on
127.0.0.1 (localhost for Microsoft), exchanges it, and writes one JSON object with the
refresh token. The token is a secret: it goes to --out, a file agenix encrypts, never to a
terminal. Used as agenix's editor it never touches the disk in the clear outside agenix's
own temporary file:

    EDITOR="nitya-consent microsoft --client-id <id> --out" \\
      agenix edit modules/secrets/nitya/microsoft-token.age

The apps come from the nitya-oauth stack (ADR-0009); see the guide's Deployment section.
"""

import argparse
import base64
import hashlib
import http.server
import json
import os
import secrets
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser

GOOGLE_AUTH = "https://accounts.google.com/o/oauth2/v2/auth"
GOOGLE_TOKEN = "https://oauth2.googleapis.com/token"
GOOGLE_SCOPES = [
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/gmail.modify",
]
MS_LOGIN = "https://login.microsoftonline.com"
MS_SCOPES = [
    "offline_access",
    "https://graph.microsoft.com/Calendars.ReadWrite",
    "https://graph.microsoft.com/Mail.ReadWrite",
    "https://graph.microsoft.com/User.Read",
]


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def pkce():
    verifier = base64.urlsafe_b64encode(secrets.token_bytes(48)).rstrip(b"=").decode()
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
    return verifier, challenge


def receive_code(host, state, timeout):
    """Serve one redirect on a free loopback port; returns (redirect_uri, wait)."""
    result = {}
    done = threading.Event()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            q = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
            if "code" not in q and "error" not in q:
                self.send_response(404)
                self.end_headers()
                return
            result.update({k: v[0] for k, v in q.items()})
            ok = "code" in q and q.get("state", [""])[0] == state
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            msg = "nitya has the code; you can close this tab." if ok else "Sign-in failed; see the terminal."
            self.wfile.write(f"<!doctype html><title>nitya</title><p>{msg}</p>".encode())
            done.set()

        def log_message(self, *args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    redirect = f"http://{host}:{server.server_address[1]}/"

    def wait():
        if not done.wait(timeout):
            server.shutdown()
            sys.exit(f"nitya-consent: no answer from the browser in {timeout} s")
        server.shutdown()
        if "error" in result:
            sys.exit(f"nitya-consent: the provider refused: {result['error']} {result.get('error_description', '')}")
        if result.get("state") != state:
            sys.exit("nitya-consent: the answer's state does not match; refusing it")
        return result["code"]

    return redirect, wait


def post_form(url, fields):
    req = urllib.request.Request(
        url,
        data=urllib.parse.urlencode(fields).encode(),
        headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")[:500]
        sys.exit(f"nitya-consent: token exchange failed: HTTP {e.code}: {body}")


def flow(args, auth_url, token_url, host, client_id, extra_auth, extra_token, scopes):
    verifier, challenge = pkce()
    state = secrets.token_urlsafe(24)
    redirect, wait = receive_code(host, state, args.timeout)
    url = auth_url + "?" + urllib.parse.urlencode({
        "client_id": client_id,
        "redirect_uri": redirect,
        "response_type": "code",
        "scope": " ".join(scopes),
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        **extra_auth,
    })
    log(f"Open this page to sign in (waiting {args.timeout} s):\n\n  {url}\n")
    if not args.no_browser:
        webbrowser.open(url)
    code = wait()
    tok = post_form(token_url, {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect,
        "client_id": client_id,
        "code_verifier": verifier,
        **extra_token,
    })
    if "refresh_token" not in tok:
        sys.exit("nitya-consent: the provider gave no refresh token (was offline access granted?)")
    return tok


def google(args):
    with open(args.client_json) as f:
        c = json.load(f)
    c = c.get("installed") or c.get("web") or c
    tok = flow(
        args,
        args.auth_url or GOOGLE_AUTH,
        args.token_url or GOOGLE_TOKEN,
        "127.0.0.1",
        c["client_id"],
        # offline access and a fresh consent, or Google gives no refresh token
        {"access_type": "offline", "prompt": "consent"},
        # a desktop client's secret is not confidential (Google's own words), but is required
        {"client_secret": c["client_secret"]},
        args.scope or GOOGLE_SCOPES,
    )
    return {"provider": "google", "client_id": c["client_id"], "token_uri": args.token_url or GOOGLE_TOKEN, **pick(tok)}


def microsoft(args):
    base = f"{MS_LOGIN}/{args.tenant}/oauth2/v2.0"
    token_uri = args.token_url or f"{base}/token"
    tok = flow(
        args,
        args.auth_url or f"{base}/authorize",
        token_uri,
        # Entra accepts any port on a registered http://localhost redirect
        "localhost",
        args.client_id,
        {"prompt": "select_account"},
        {},
        args.scope or MS_SCOPES,
    )
    return {"provider": "microsoft", "client_id": args.client_id, "tenant": args.tenant, "token_uri": token_uri, **pick(tok)}


def pick(tok):
    return {
        "refresh_token": tok["refresh_token"],
        "scope": tok.get("scope", ""),
        "obtained_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }


def main():
    p = argparse.ArgumentParser(
        prog="nitya-consent",
        description="Sign in to Google or Microsoft once and write a refresh token for nitya as JSON.",
        epilog='As agenix\'s editor: EDITOR="nitya-consent microsoft --client-id <id> --out" agenix edit <secret>.age',
    )
    sub = p.add_subparsers(dest="provider", required=True)
    g = sub.add_parser("google", help="a Google account, through the Desktop client made in the console")
    g.add_argument("--client-json", required=True, help="the client's JSON as downloaded from the console")
    m = sub.add_parser("microsoft", help="a Microsoft account, through the Entra app of the nitya-oauth stack")
    m.add_argument("--client-id", required=True, help="the stack's microsoft_client_id output")
    m.add_argument("--tenant", default="common", help="'common' (default), 'consumers', 'organizations' or a tenant id")
    for s in (g, m):
        s.add_argument("--scope", action="append", help="a scope to ask for, repeatable (default: what nitya needs)")
        s.add_argument("--no-browser", action="store_true", help="only print the sign-in URL (another machine's browser)")
        s.add_argument("--timeout", type=int, default=300, help="seconds to wait for the browser (default 300)")
        s.add_argument("--auth-url", help=argparse.SUPPRESS)  # tests point these at a fake provider
        s.add_argument("--token-url", help=argparse.SUPPRESS)
        # Last, so that `EDITOR="nitya-consent … --out"` gets the path agenix appends.
        s.add_argument("--out", help="write the JSON here (mode 0600); without it, stdout, which must not be a terminal")
    args = p.parse_args()
    if not args.out and sys.stdout.isatty():
        sys.exit("nitya-consent: refusing to print a refresh token to a terminal; use --out or a pipe")
    result = google(args) if args.provider == "google" else microsoft(args)
    text = json.dumps(result, indent=2) + "\n"
    if args.out:
        fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(text)
        log(f"nitya-consent: {result['provider']} refresh token written to {args.out}")
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
