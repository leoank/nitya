"""nitya-token-probe: do the owner's provider tokens keep working on a headless server?

    nitya-token-probe check --state <dir> [--google-client F --google-token F]
                            [--microsoft-token F] [--icloud F]
    nitya-token-probe report <dir/probe.jsonl>

`check` refreshes each token it is given, makes one small read with it (Google: the
calendar list and the latest Gmail message; Microsoft: the calendars and the latest
message; iCloud: the CalDAV principal), and appends one JSON line per check to
<state>/probe.jsonl. It never writes a token or a message to the log: only whether it
worked, the provider's error code when not, counts, and a short hash of the latest
message id (to show the mailbox is really read).

Microsoft rotates refresh tokens: every refresh returns a new one. Two chains are
checked: `latest` keeps each new token in <state>/microsoft-latest.json (mode 0600), as
nitya's server would; `original` refreshes the token it was given every time, to show
whether the bootstrap token alone would have lasted. Google's token does not rotate.

`report` prints, per provider and chain, the checks made, the first and last success,
and every failure. EXP-20261123-bhfz49 runs this every 6 hours on ghost001 for 30 days.
"""

import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

# Every endpoint, overridable as a whole by NITYA_PROBE_ENDPOINTS (JSON) for the test.
ENDPOINTS = {
    "google_token": "https://oauth2.googleapis.com/token",
    "google_calendars": "https://www.googleapis.com/calendar/v3/users/me/calendarList?fields=items(id)",
    "gmail_latest": "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults=1",
    "microsoft_token": None,  # the token file's token_uri
    "graph_calendars": "https://graph.microsoft.com/v1.0/me/calendars?$select=id",
    "graph_latest": "https://graph.microsoft.com/v1.0/me/messages?$top=1&$select=id",
    "icloud": "https://caldav.icloud.com/",
}
ENDPOINTS.update(json.loads(os.environ.get("NITYA_PROBE_ENDPOINTS", "{}")))


class Failed(Exception):
    def __init__(self, step, status, code):
        super().__init__(f"{step}: {status} {code}")
        self.step, self.status, self.code = step, status, code


def request(step, url, method="GET", form=None, headers=None, body=None):
    data = urllib.parse.urlencode(form).encode() if form is not None else body
    h = {"Accept": "application/json", **(headers or {})}
    if form is not None:
        h["Content-Type"] = "application/x-www-form-urlencoded"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, method=method, headers=h), timeout=60) as r:
            raw = r.read()
            return json.loads(raw) if raw and r.headers.get_content_subtype() == "json" else {}
    except urllib.error.HTTPError as e:
        try:  # the provider's error code only ("invalid_grant"), never the whole body
            j = json.loads(e.read())
            code = j.get("error") if isinstance(j.get("error"), str) else (j.get("error") or {}).get("code") or (j.get("error") or {}).get("status")
        except Exception:
            code = None
        raise Failed(step, e.code, code or "")
    except (urllib.error.URLError, TimeoutError) as e:
        raise Failed(step, "network", type(e).__name__)


def short(s):
    return hashlib.sha256(s.encode()).hexdigest()[:8] if s else None


def load(path):
    with open(path) as f:
        return json.load(f)


def google(client_file, token_file):
    c = load(client_file)
    c = c.get("installed") or c.get("web") or c
    t = load(token_file)
    tok = request("refresh", ENDPOINTS["google_token"], "POST", form={
        "grant_type": "refresh_token", "refresh_token": t["refresh_token"],
        "client_id": c["client_id"], "client_secret": c["client_secret"]})
    auth = {"Authorization": f"Bearer {tok['access_token']}"}
    cals = request("calendars", ENDPOINTS["google_calendars"], headers=auth).get("items", [])
    msgs = request("gmail", ENDPOINTS["gmail_latest"], headers=auth).get("messages", [])
    return {"calendars": len(cals), "latest": short(msgs[0]["id"] if msgs else "")}


def microsoft(token_file, refresh_token, save=None):
    t = load(token_file)
    tok = request("refresh", ENDPOINTS["microsoft_token"] or t["token_uri"], "POST", form={
        "grant_type": "refresh_token", "refresh_token": refresh_token, "client_id": t["client_id"],
        "scope": "https://graph.microsoft.com/.default offline_access"})
    rotated = bool(tok.get("refresh_token")) and tok["refresh_token"] != refresh_token
    if save and tok.get("refresh_token"):
        fd = os.open(save + ".tmp", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            json.dump({"refresh_token": tok["refresh_token"]}, f)
        os.replace(save + ".tmp", save)
    auth = {"Authorization": f"Bearer {tok['access_token']}"}
    cals = request("calendars", ENDPOINTS["graph_calendars"], headers=auth).get("value", [])
    msgs = request("mail", ENDPOINTS["graph_latest"], headers=auth).get("value", [])
    return {"calendars": len(cals), "latest": short(msgs[0]["id"] if msgs else ""), "rotated": rotated}


def icloud(file):
    t = load(file)
    auth = "Basic " + base64.b64encode(f"{t['username']}:{t['app_password']}".encode()).decode()
    body = b'<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>'
    headers = {"Authorization": auth, "Depth": "0", "Content-Type": "application/xml"}
    request("principal", ENDPOINTS["icloud"], "PROPFIND", headers=headers, body=body)
    return {}


def check(a):
    os.makedirs(a.state, exist_ok=True)
    log = os.path.join(a.state, "probe.jsonl")
    now = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    jobs = []
    if a.google_client and a.google_token:
        jobs.append(("google", "token", lambda: google(a.google_client, a.google_token)))
    if a.microsoft_token:
        latest = os.path.join(a.state, "microsoft-latest.json")
        original = load(a.microsoft_token)["refresh_token"]
        current = load(latest)["refresh_token"] if os.path.exists(latest) else original
        jobs.append(("microsoft", "latest", lambda: microsoft(a.microsoft_token, current, save=latest)))
        jobs.append(("microsoft", "original", lambda: microsoft(a.microsoft_token, original)))
    if a.icloud:
        jobs.append(("icloud", "app-password", lambda: icloud(a.icloud)))
    failed = 0
    with open(log, "a") as f:
        for provider, chain, job in jobs:
            line = {"t": now, "provider": provider, "chain": chain}
            try:
                line.update(ok=True, **job())
            except Failed as e:
                failed += 1
                line.update(ok=False, step=e.step, status=e.status, error=e.code)
            except (KeyError, OSError, ValueError) as e:
                failed += 1
                line.update(ok=False, step="local", status="local", error=type(e).__name__)
            f.write(json.dumps(line) + "\n")
            print(json.dumps(line))
    return 1 if failed else 0


def report(a):
    runs = {}
    with open(a.log) as f:
        for raw in f:
            x = json.loads(raw)
            r = runs.setdefault((x["provider"], x["chain"]), {"checks": 0, "ok": 0, "first_ok": None, "last_ok": None, "failures": []})
            r["checks"] += 1
            if x["ok"]:
                r["ok"] += 1
                r["first_ok"] = r["first_ok"] or x["t"]
                r["last_ok"] = x["t"]
            else:
                r["failures"].append(f"{x['t']} {x['step']} {x['status']} {x['error']}")
    for (p, c), r in sorted(runs.items()):
        days = None
        if r["first_ok"] and r["last_ok"]:
            span = dt.datetime.fromisoformat(r["last_ok"][:-1]) - dt.datetime.fromisoformat(r["first_ok"][:-1])
            days = round(span.total_seconds() / 86400, 2)
        print(f"{p}\t{c}\tchecks={r['checks']}\tok={r['ok']}\tfirst_ok={r['first_ok']}\tlast_ok={r['last_ok']}\tdays_ok={days}\tfailures={len(r['failures'])}")
        for x in r["failures"]:
            print(f"  {x}")
    allok = all(not r["failures"] for r in runs.values())
    shortest = min((round((dt.datetime.fromisoformat(r["last_ok"][:-1]) - dt.datetime.fromisoformat(r["first_ok"][:-1])).total_seconds() / 86400, 2)
                   for r in runs.values() if r["first_ok"]), default=0)
    print("===RESULT=== " + json.dumps({"chains": len(runs), "all_ok": allok, "min_days_ok": shortest,
                                       **{f"{p}_{c}_failures": len(r["failures"]) for (p, c), r in sorted(runs.items())}}))


def main():
    p = argparse.ArgumentParser(prog="nitya-token-probe", description="Check that provider tokens still work; log one JSON line per check.")
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check", help="refresh each token, read a little, append to <state>/probe.jsonl")
    c.add_argument("--state", required=True, help="directory for probe.jsonl and Microsoft's latest token")
    c.add_argument("--google-client", help="the Google Desktop client JSON")
    c.add_argument("--google-token", help="nitya-consent's Google token JSON")
    c.add_argument("--microsoft-token", help="nitya-consent's Microsoft token JSON")
    c.add_argument("--icloud", help='{"username": ..., "app_password": ...}')
    r = sub.add_parser("report", help="summarise a probe.jsonl")
    r.add_argument("log")
    a = p.parse_args()
    sys.exit(check(a) if a.cmd == "check" else report(a))


if __name__ == "__main__":
    main()
