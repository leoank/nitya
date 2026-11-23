"""Writes the corpus for EXP-20261123-bhfzfs: one hard case per .ics file.
Run from this directory: python3 gen.py. The files are committed; this script is
how they were made, so a case is changed here and regenerated."""
from pathlib import Path

TZ = {
"Europe/London": """BEGIN:VTIMEZONE
TZID:Europe/London
BEGIN:DAYLIGHT
TZOFFSETFROM:+0000
TZOFFSETTO:+0100
TZNAME:BST
DTSTART:19700329T010000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:+0100
TZOFFSETTO:+0000
TZNAME:GMT
DTSTART:19701025T020000
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU
END:STANDARD
END:VTIMEZONE""",
"America/New_York": """BEGIN:VTIMEZONE
TZID:America/New_York
BEGIN:DAYLIGHT
TZOFFSETFROM:-0500
TZOFFSETTO:-0400
TZNAME:EDT
DTSTART:19700308T020000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:-0400
TZOFFSETTO:-0500
TZNAME:EST
DTSTART:19701101T020000
RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU
END:STANDARD
END:VTIMEZONE""",
"Asia/Kolkata": """BEGIN:VTIMEZONE
TZID:Asia/Kolkata
BEGIN:STANDARD
TZOFFSETFROM:+0530
TZOFFSETTO:+0530
TZNAME:IST
DTSTART:19700101T000000
END:STANDARD
END:VTIMEZONE""",
}
STAMP = "DTSTAMP:20261123T120000Z"
LDN = "TZID=Europe/London"

def ev(uid, *lines):
    return "\n".join(["BEGIN:VEVENT", f"UID:{uid}@nitya-corpus", STAMP, *lines, "END:VEVENT"])

def cal(*events, tz=()):
    body = [TZ[t] for t in tz] + list(events)
    return "\n".join(["BEGIN:VCALENDAR", "VERSION:2.0",
                      "PRODID:-//nitya//EXP-20261123-bhfzfs corpus//EN", *body, "END:VCALENDAR"])

def fold(text):
    """CRLF line ends and 75-octet folding, as RFC 5545 section 3.1 requires."""
    out = []
    for line in text.split("\n"):
        b = line.encode()
        while len(b) > 75:
            cut = 75
            while (b[cut] & 0xC0) == 0x80:  # never split a UTF-8 sequence
                cut -= 1
            out.append(b[:cut].decode()); b = b" " + b[cut:]
        out.append(b.decode())
    return "\r\n".join(out) + "\r\n"

C = {}
C["01-timed-london"] = cal(ev("c01", f"DTSTART;{LDN}:20270114T100000", f"DTEND;{LDN}:20270114T110000",
    "SUMMARY:Timed event in the calendar's zone"), tz=["Europe/London"])
C["02-all-day"] = cal(ev("c02", "DTSTART;VALUE=DATE:20270120", "DTEND;VALUE=DATE:20270121",
    "SUMMARY:All-day event", "TRANSP:TRANSPARENT"))
C["03-multi-day-all-day"] = cal(ev("c03", "DTSTART;VALUE=DATE:20270201", "DTEND;VALUE=DATE:20270204",
    "SUMMARY:Three-day all-day event"))
C["04-floating"] = cal(ev("c04", "DTSTART:20270215T090000", "DTEND:20270215T093000",
    "SUMMARY:Floating time (no zone)"))
C["05-utc"] = cal(ev("c05", "DTSTART:20270216T150000Z", "DTEND:20270216T160000Z",
    "SUMMARY:UTC event"))
C["06-weekly-count"] = cal(ev("c06", f"DTSTART;{LDN}:20270104T090000", f"DTEND;{LDN}:20270104T093000",
    "RRULE:FREQ=WEEKLY;COUNT=10;BYDAY=MO", "SUMMARY:Weekly\\, ten times"), tz=["Europe/London"])
C["07-weekly-exdate"] = cal(ev("c07", f"DTSTART;{LDN}:20270105T140000", f"DTEND;{LDN}:20270105T150000",
    "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20270330T140000Z",
    f"EXDATE;{LDN}:20270119T140000,20270202T140000", "SUMMARY:Weekly with two dates excluded"),
    tz=["Europe/London"])
C["08-daily-moved-instance"] = cal(
    ev("c08", f"DTSTART;{LDN}:20270301T080000", f"DTEND;{LDN}:20270301T083000",
       "RRULE:FREQ=DAILY;COUNT=7", "SUMMARY:Daily standup"),
    ev("c08", f"RECURRENCE-ID;{LDN}:20270303T080000", f"DTSTART;{LDN}:20270303T113000",
       f"DTEND;{LDN}:20270303T120000", "SUMMARY:Daily standup (moved to late morning)", "SEQUENCE:1"),
    tz=["Europe/London"])
C["09-cancelled-instance"] = cal(
    ev("c09", f"DTSTART;{LDN}:20270308T170000", f"DTEND;{LDN}:20270308T180000",
       "RRULE:FREQ=WEEKLY;COUNT=6", "SUMMARY:Weekly with one instance cancelled"),
    ev("c09", f"RECURRENCE-ID;{LDN}:20270322T170000", f"DTSTART;{LDN}:20270322T170000",
       f"DTEND;{LDN}:20270322T180000", "SUMMARY:Weekly with one instance cancelled",
       "STATUS:CANCELLED", "SEQUENCE:1"),
    tz=["Europe/London"])
C["10a-thisandfuture-range"] = cal(
    ev("c10a", f"DTSTART;{LDN}:20270405T100000", f"DTEND;{LDN}:20270405T110000",
       "RRULE:FREQ=WEEKLY;COUNT=8", "SUMMARY:Series changed this-and-following by RANGE"),
    ev("c10a", f"RECURRENCE-ID;RANGE=THISANDFUTURE;{LDN}:20270503T100000",
       f"DTSTART;{LDN}:20270503T150000", f"DTEND;{LDN}:20270503T160000",
       "SUMMARY:Series changed this-and-following by RANGE (afternoons)", "SEQUENCE:1"),
    tz=["Europe/London"])
C["10b-split-series"] = cal(
    ev("c10b-1", f"DTSTART;{LDN}:20270406T100000", f"DTEND;{LDN}:20270406T110000",
       "RRULE:FREQ=WEEKLY;UNTIL=20270427T090000Z", "SUMMARY:Series split this-and-following (first part)"),
    ev("c10b-2", f"DTSTART;{LDN}:20270504T150000", f"DTEND;{LDN}:20270504T160000",
       "RRULE:FREQ=WEEKLY;COUNT=4", "RELATED-TO;RELTYPE=SIBLING:c10b-1@nitya-corpus",
       "SUMMARY:Series split this-and-following (second part)"),
    tz=["Europe/London"])
C["11-weekly-across-dst"] = cal(ev("c11", f"DTSTART;{LDN}:20270315T090000", f"DTEND;{LDN}:20270315T100000",
    "RRULE:FREQ=WEEKLY;COUNT=5", "SUMMARY:Weekly at 09:00 London across the March DST change"),
    tz=["Europe/London"])
C["12-other-zone-new-york"] = cal(ev("c12", "DTSTART;TZID=America/New_York:20270310T090000",
    "DTEND;TZID=America/New_York:20270310T100000", "RRULE:FREQ=DAILY;COUNT=7",
    "SUMMARY:New York daily across the US DST change (London calendar)"), tz=["America/New_York"])
C["13-half-hour-zone"] = cal(ev("c13", "DTSTART;TZID=Asia/Kolkata:20270412T183000",
    "DTEND;TZID=Asia/Kolkata:20270412T193000", "SUMMARY:Kolkata evening (+05:30)"), tz=["Asia/Kolkata"])
C["14-monthly-last-friday"] = cal(ev("c14", f"DTSTART;{LDN}:20270129T160000", f"DTEND;{LDN}:20270129T170000",
    "RRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=12", "SUMMARY:Last Friday of the month"), tz=["Europe/London"])
C["15-monthly-bysetpos"] = cal(ev("c15", f"DTSTART;{LDN}:20270129T120000", f"DTEND;{LDN}:20270129T123000",
    "RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=12",
    "SUMMARY:Last weekday of the month (BYSETPOS)"), tz=["Europe/London"])
C["16-rdate"] = cal(ev("c16", f"DTSTART;{LDN}:20270601T100000", f"DTEND;{LDN}:20270601T110000",
    "RRULE:FREQ=WEEKLY;COUNT=4", f"RDATE;{LDN}:20270605T100000,20270612T100000",
    "SUMMARY:Weekly plus two extra dates (RDATE)"), tz=["Europe/London"])
C["17-yearly-all-day"] = cal(ev("c17", "DTSTART;VALUE=DATE:20270707", "DTEND;VALUE=DATE:20270708",
    "RRULE:FREQ=YEARLY", "SUMMARY:Birthday", "TRANSP:TRANSPARENT"))
C["18-attendees"] = cal(ev("c18", f"DTSTART;{LDN}:20270707T150000", f"DTEND;{LDN}:20270707T160000",
    "SUMMARY:Meeting with every participation status",
    "ORGANIZER;CN=Owner:mailto:owner@example.org",
    "ATTENDEE;CN=Owner;ROLE=CHAIR;PARTSTAT=ACCEPTED:mailto:owner@example.org",
    "ATTENDEE;CN=Accepts;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:a@example.org",
    "ATTENDEE;CN=Declines;ROLE=REQ-PARTICIPANT;PARTSTAT=DECLINED;RSVP=TRUE:mailto:d@example.org",
    "ATTENDEE;CN=Maybe;ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE;RSVP=TRUE:mailto:t@example.org",
    "ATTENDEE;CN=No answer;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:n@example.org",
    "ATTENDEE;CN=Room;CUTYPE=ROOM;ROLE=NON-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:room@example.org"),
    tz=["Europe/London"])
C["19-long-unicode-text"] = cal(ev("c19", f"DTSTART;{LDN}:20270708T100000", f"DTEND;{LDN}:20270708T110000",
    "SUMMARY:Ünïcödé\\, commas\\, semicolons\\; and a backslash \\\\ — नित्य",
    "DESCRIPTION:" + "A long description that has to be folded across several lines. " * 4
        + "\\nSecond paragraph after an escaped newline\\, with नित्य and emoji 📅.",
    "LOCATION:Room 1\\, Building 2\\; Floor 3"), tz=["Europe/London"])
C["20-attach-url-x-props"] = cal(ev("c20", f"DTSTART;{LDN}:20270709T100000", f"DTEND;{LDN}:20270709T110000",
    "SUMMARY:Attachment by URL\\, a URL and vendor properties",
    "ATTACH;FMTTYPE=application/pdf:https://example.org/agenda.pdf",
    "URL:https://example.org/meeting", "CATEGORIES:work,planning",
    "X-NITYA-TEST:kept-or-dropped", "X-MICROSOFT-CDO-BUSYSTATUS:TENTATIVE"), tz=["Europe/London"])
C["21-alarms"] = cal(ev("c21", f"DTSTART;{LDN}:20270710T100000", f"DTEND;{LDN}:20270710T110000",
    "SUMMARY:Two reminders",
    "BEGIN:VALARM", "ACTION:DISPLAY", "DESCRIPTION:Reminder", "TRIGGER:-PT15M", "END:VALARM",
    "BEGIN:VALARM", "ACTION:DISPLAY", "DESCRIPTION:Reminder", "TRIGGER;RELATED=START:-P1D", "END:VALARM"),
    tz=["Europe/London"])
C["22-all-day-series-timed-override"] = cal(
    ev("c22", "DTSTART;VALUE=DATE:20270802", "DTEND;VALUE=DATE:20270803",
       "RRULE:FREQ=WEEKLY;COUNT=4", "SUMMARY:Weekly all-day\\, one instance made timed"),
    ev("c22", "RECURRENCE-ID;VALUE=DATE:20270809", f"DTSTART;{LDN}:20270809T140000",
       f"DTEND;{LDN}:20270809T150000", "SUMMARY:Weekly all-day\\, one instance made timed", "SEQUENCE:1"),
    tz=["Europe/London"])
C["23-private-busy-status"] = cal(ev("c23", f"DTSTART;{LDN}:20270711T100000", f"DTEND;{LDN}:20270711T110000",
    "SUMMARY:Private and tentative", "CLASS:PRIVATE", "STATUS:TENTATIVE", "TRANSP:OPAQUE",
    "PRIORITY:1"), tz=["Europe/London"])

out = Path(__file__).parent
for name, text in C.items():
    (out / f"{name}.ics").write_bytes(fold(text).encode())
print(f"{len(C)} cases")
