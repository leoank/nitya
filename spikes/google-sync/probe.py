"""EXP-20261127-mqzh92: Google Calendar's incremental sync and conditional writes.

Makes a throwaway calendar on the owner's account, changes it the ways a channel sees,
and records what `events.list` with a sync token reports, how a garbled token is
answered, and whether `If-Match` refuses stale writes. Writes <out>/listings.json
(only the fields the driver reads; the calendar's id and the owner's address replaced)
and <out>/table.tsv, one row per prediction. The calendar is deleted at the end.

    python3 -I probe.py <client json> <token json> <out>

The client is the Desktop client JSON, the token nitya-consent's JSON. Nothing secret
is printed or written.
"""

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

G = "https://www.googleapis.com/calendar/v3"
KEEP = ("id", "status", "recurringEventId", "originalStartTime", "summary", "start", "iCalUID", "recurrence")


class Http(Exception):
    def __init__(self, code, body):
        super().__init__(f"{code} {body[:200]}")
        self.code = code
        self.body = body


def http(method, url, tok=None, body=None, form=None, headers=None):
    h = dict(headers or {})
    data = None
    if tok:
        h["Authorization"] = f"Bearer {tok}"
    if body is not None:
        data = json.dumps(body).encode()
        h["Content-Type"] = "application/json"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        h["Content-Type"] = "application/x-www-form-urlencoded"
    req = urllib.request.Request(url, data=data, method=method, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raise Http(e.code, e.read().decode(errors="replace")) from None


def main():
    client_path, token_path, out = sys.argv[1:4]
    os.makedirs(out, exist_ok=True)
    with open(client_path) as f:
        c = json.load(f)["installed"]
    with open(token_path) as f:
        t = json.load(f)
    tok = http("POST", c["token_uri"], form={"grant_type": "refresh_token", "refresh_token": t["refresh_token"],
                                             "client_id": c["client_id"], "client_secret": c["client_secret"]})["access_token"]
    owner = http("GET", f"{G}/calendars/primary", tok)["id"]
    cal = http("POST", f"{G}/calendars", tok, {"summary": "nitya-sync-probe", "timeZone": "Europe/London"})["id"]
    ev = f"{G}/calendars/{urllib.parse.quote(cal)}/events"

    def red(x):
        s = json.dumps(x)
        return json.loads(s.replace(cal, "probe-calendar@example.invalid").replace(owner, "owner@example.invalid"))

    def listing(sync=None, page=2500):
        pages, items, pt = 0, [], None
        token_pages = []
        while True:
            q = {"maxResults": page, "singleEvents": "false"}
            if sync:
                q["syncToken"] = sync
            if pt:
                q["pageToken"] = pt
            r = http("GET", f"{ev}?{urllib.parse.urlencode(q)}", tok)
            pages += 1
            items += r.get("items", [])
            token_pages.append("nextSyncToken" in r)
            pt = r.get("nextPageToken")
            if not pt:
                return items, r.get("nextSyncToken"), pages, token_pages

    def short(items):
        return [{k: i[k] for k in KEEP if k in i} for i in items]

    rows, record = [], {}
    try:
        def when(d, h):
            return {"dateTime": f"2027-03-{d:02d}T{h:02d}:00:00", "timeZone": "Europe/London"}

        a = http("POST", ev, tok, {"summary": "single A", "start": when(1, 9), "end": when(1, 10)})
        b = http("POST", ev, tok, {"summary": "single B", "start": when(2, 9), "end": when(2, 10)})
        s = http("POST", ev, tok, {"summary": "weekly S", "start": when(3, 9), "end": when(3, 10),
                                   "recurrence": ["RRULE:FREQ=WEEKLY;COUNT=6"]})
        inst = http("GET", f"{ev}/{s['id']}/instances", tok)["items"]
        moved = dict(inst[1], start=when(11, 14), end=when(11, 15))
        http("PUT", f"{ev}/{inst[1]['id']}", tok, moved)
        http("DELETE", f"{ev}/{inst[2]['id']}", tok)

        # 1. a full listing, in pages of 2
        full, sync1, pages, token_pages = listing(page=2)
        record["full"] = short(full)
        kinds = sorted(("cancelled-instance" if i.get("status") == "cancelled" and "recurringEventId" in i
                        else "instance" if "recurringEventId" in i else "event") for i in full)
        ok1 = (kinds == ["cancelled-instance", "event", "event", "event", "instance"]
               and token_pages[-1] and not any(token_pages[:-1]) and pages > 1)
        rows.append(("1 full listing: masters, moved and cancelled instances; token on the last page only", ok1,
                     f"items {kinds}; pages {pages}; token on pages {token_pages}"))

        # 2. edits, then the token's listing
        a = http("PATCH", f"{ev}/{a['id']}", tok, {"summary": "single A retitled"})
        moved2 = http("GET", f"{ev}/{s['id']}/instances", tok)["items"]
        m3 = next(i for i in moved2 if i["start"]["dateTime"].startswith("2027-03-24"))
        http("PUT", f"{ev}/{m3['id']}", tok, dict(m3, start=when(25, 9), end=when(25, 10)))
        m4 = next(i for i in moved2 if i["start"]["dateTime"].startswith("2027-03-31"))
        http("DELETE", f"{ev}/{m4['id']}", tok)
        http("DELETE", f"{ev}/{b['id']}", tok)
        c_ = http("POST", ev, tok, {"summary": "single C", "start": when(4, 9), "end": when(4, 10)})
        inc, sync2, _, _ = listing(sync1)
        record["incremental"] = short(inc)
        got = {i["id"]: i for i in inc}
        ok2 = (set(got) == {a["id"], m3["id"], m4["id"], b["id"], c_["id"]}
               and got[b["id"]]["status"] == "cancelled" and "recurringEventId" not in got[b["id"]]
               and got[m4["id"]]["status"] == "cancelled" and got[m4["id"]].get("recurringEventId") == s["id"]
               and got[m3["id"]].get("recurringEventId") == s["id"] and got[m3["id"]]["status"] == "confirmed")
        again, _, _, _ = listing(sync2)
        record["incremental_again"] = short(again)
        rows.append(("2 the token lists exactly the five changes; deleted as cancelled; newest token empty",
                     ok2 and not again,
                     f"{len(inc)} items: " + "; ".join(f"{i.get('summary', '-')}/{i['status']}/{'instance' if 'recurringEventId' in i else 'event'}" for i in inc)
                     + f"; then {len(again)}"))

        # 3. a garbled token
        try:
            listing(sync2[:-6] + "garble")
            code = 200
        except Http as e:
            code = e.code
            record["garbled"] = red(json.loads(e.body)) if e.body.startswith("{") else e.body[:300]
        rows.append(("3 a garbled token is answered 410", code == 410, f"status {code}"))

        # 4. If-Match on update and delete
        cur = http("GET", f"{ev}/{c_['id']}", tok)
        stale = cur["etag"]
        cur = http("PATCH", f"{ev}/{c_['id']}", tok, {"summary": "single C v2"})
        res = {}
        for name, method, body in (("update", "PUT", dict(cur, summary="from a stale copy")), ("delete", "DELETE", None)):
            try:
                http(method, f"{ev}/{c_['id']}", tok, body, headers={"If-Match": stale})
                res[name + "_stale"] = 200
            except Http as e:
                res[name + "_stale"] = e.code
        after = http("GET", f"{ev}/{c_['id']}", tok)
        res["unchanged"] = after.get("summary") == "single C v2" and after.get("status") == "confirmed"
        upd = http("PUT", f"{ev}/{c_['id']}", tok, dict(after, summary="single C v3"), headers={"If-Match": after["etag"]})
        res["update_current"] = upd.get("summary") == "single C v3"
        http("DELETE", f"{ev}/{c_['id']}", tok, headers={"If-Match": upd["etag"]})
        res["delete_current"] = http("GET", f"{ev}/{c_['id']}", tok).get("status") == "cancelled"
        record["if_match"] = res
        rows.append(("4 If-Match: stale update and delete 412 and change nothing; current ones succeed",
                     res["update_stale"] == 412 and res["delete_stale"] == 412 and res["unchanged"]
                     and res["update_current"] and res["delete_current"], json.dumps(res)))

        # 5. insert with an iCalUID
        uid = "nitya-probe-0001@nitya.invalid"
        d = http("POST", ev, tok, {"summary": "with uid", "iCalUID": uid, "start": when(5, 9), "end": when(5, 10)})
        rows.append(("5 insert keeps the iCalUID", d.get("iCalUID") == uid, f"iCalUID kept: {d.get('iCalUID') == uid}"))
    finally:
        http("DELETE", f"{G}/calendars/{urllib.parse.quote(cal)}", tok)

    with open(os.path.join(out, "listings.json"), "w") as f:
        json.dump(red(record), f, indent=1)
    with open(os.path.join(out, "table.tsv"), "w") as f:
        f.write("prediction\theld\tobserved\n")
        for p, ok, obs in rows:
            f.write(f"{p}\t{int(bool(ok))}\t{red(obs)}\n")
    for p, ok, obs in rows:
        print(f"{'held ' if ok else 'FAILED'}  {p}: {red(obs)}")
    held = sum(1 for r in rows if r[1])
    print('===RESULT=== ' + json.dumps({"held": held, "predictions": len(rows)}))


if __name__ == "__main__":
    main()
