## 04-floating / import
- c04@nitya-corpus | rid F20270215T090000 | floating 2027-02-15T09:00 | 1800s | summary="Floating time (no zone)" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[] url= attach=[] cat=
+ c04@nitya-corpus | rid U20270215T090000Z | Europe/London 2027-02-15T09:00 (09:00Z) | 1800s | summary="Floating time (no zone)" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[] url= attach=[] cat=

## 09-cancelled-instance / import
- c09@nitya-corpus | rid U20270322T170000Z | Europe/London 2027-03-22T17:00 (17:00Z) | 3600s | summary="Weekly with one instance cancelled" desc="" loc="" status=CANCELLED transp=OPAQUE class=PUBLIC org= att=[] alarms=[] url= attach=[] cat=

## 10a-thisandfuture-range / import
refused before writing: RECURRENCE-ID;RANGE=THISANDFUTURE: Graph has no range; a driver must split the series (case 10b)

## 16-rdate / import
refused before writing: RDATE: Graph's recurrence has no extra dates

## 20-attach-url-x-props / import
- c20@nitya-corpus | rid U20270709T090000Z | Europe/London 2027-07-09T10:00 (09:00Z) | 3600s | summary="Attachment by URL, a URL and vendor properties" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[] url=https://example.org/meeting attach=["https://example.org/agenda.pdf"] cat=work,planning
+ c20@nitya-corpus | rid U20270709T090000Z | Europe/London 2027-07-09T10:00 (09:00Z) | 3600s | summary="Attachment by URL, a URL and vendor properties" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[] url= attach=[] cat=work,planning

## 21-alarms / import
- c21@nitya-corpus | rid U20270710T090000Z | Europe/London 2027-07-10T10:00 (09:00Z) | 3600s | summary="Two reminders" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[DISPLAY-P1D,DISPLAY-PT15M] url= attach=[] cat=
+ c21@nitya-corpus | rid U20270710T090000Z | Europe/London 2027-07-10T10:00 (09:00Z) | 3600s | summary="Two reminders" desc="" loc="" status=CONFIRMED transp=OPAQUE class=PUBLIC org= att=[] alarms=[DISPLAY-PT15M] url= attach=[] cat=

## 18-attendees / itip-reply
{"invite_reached_google_s": 0, "reply_reached_graph_s": null}

