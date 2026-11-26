#!/usr/bin/env bash
# RUN-20261126-hw6442 for EXP-20261126-hvzed5: what the server learns about a caller from
# its tailnet connection, and what of it survives a restart and a fresh join under the
# same hostname. Four calls to one server: node A joins; A restarts from the same state;
# node B joins with a fresh state and A's hostname; A calls once more.
# Everything joins as ephemeral nodes on the tailnet of the auth key in $AUTHKEY_FILE.
# Node ids, node keys, addresses and the tailnet's DNS suffix are replaced by placeholders
# (id-1, key-1, addr-1, <tailnet>) before anything is written: the repo is public.
# From the repo root:
#   AUTHKEY_FILE=~/.config/sandhi/leoank-authkey nix develop -c spikes/tailnet-identity/run.sh
set -euo pipefail
here=spikes/tailnet-identity
out=${YAJNA_RUN_DIR:-$here/out}
key=${AUTHKEY_FILE:?the path of a reusable auth key file}
rm -rf "${out:?}" && mkdir -p "$out"
{ uname -sm; rustc --version; } > "$out/host.txt"

cargo build -q --locked --manifest-path "$here/Cargo.toml"
bin=$here/target/debug/tailnet-identity
state=$(mktemp -d)
calls=$state/calls.jsonl
server_pid=
cleanup() {
  [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null && wait "$server_pid" 2>/dev/null
  rm -rf "${state:?}"
}
trap cleanup EXIT

"$bin" server "$state/server" nitya-idtest "$key" 7301 > "$calls" 2> "$state/server.log" &
server_pid=$!
for _ in $(seq 1 90); do grep -q "server up" "$state/server.log" && break; sleep 1; done
grep -q "server up" "$state/server.log" || { echo "server did not come up" >&2; exit 1; }

# One call: a node joins (or resumes from its state) and dials; wait for the server's line.
call() { # label dir hostname
  local n; n=$(wc -l < "$calls")
  "$bin" node "$state/$2" "$3" "$key" nitya-idtest:7301 2> "$state/node-$1.log"
  for _ in $(seq 1 20); do [ "$(wc -l < "$calls")" -gt "$n" ] && break; sleep 0.5; done
  echo "$1" >> "$state/labels"
}
call a-first a nitya-idtest-node
call a-restart a nitya-idtest-node
call b-fresh b nitya-idtest-node
call a-after-b a nitya-idtest-node

python3 - "$calls" "$state/labels" "$out" <<'EOF'
import json, sys
calls = [json.loads(l) for l in open(sys.argv[1])]
labels = open(sys.argv[2]).read().split()
out = sys.argv[3]
assert len(calls) == len(labels), (len(calls), labels)
names = {}
def ph(kind, v):
    if not v:
        return v
    return names.setdefault((kind, v), f"{kind}-{sum(1 for k in names if k[0] == kind) + 1}")
rows = []
for label, c in zip(labels, calls):
    dns = c["dns_name"] or ""
    rows.append({
        "call": label,
        "host": c["host"],
        "login": c["login"],
        "dns_name": dns.split(".")[0] + ".<tailnet>" if dns else dns,
        "node_id": ph("id", c["node_id"]),
        "node_key": ph("key", c["node_key"]),
        "addr": ph("addr", c["addr"]),
    })
with open(f"{out}/calls.jsonl", "w") as f:
    for r in rows:
        f.write(json.dumps(r) + "\n")
by = {r["call"]: r for r in rows}
same = lambda a, b, k: by[a][k] == by[b][k]
res = {
    "p1_id_and_key_given": all(r["node_id"] and r["node_key"] for r in rows),
    "p2_restart_same_id": same("a-first", "a-restart", "node_id"),
    "p2_restart_same_key": same("a-first", "a-restart", "node_key"),
    "p2_restart_same_addr": same("a-first", "a-restart", "addr"),
    "p3_fresh_join_new_id": not same("a-first", "b-fresh", "node_id"),
    "p3_fresh_join_new_key": not same("a-first", "b-fresh", "node_key"),
    "p3_fresh_join_same_host": same("a-first", "b-fresh", "host"),
    "p3_fresh_join_same_dns": same("a-first", "b-fresh", "dns_name"),
    "a_after_b_same_id": same("a-first", "a-after-b", "node_id"),
    "a_after_b_same_dns": same("a-first", "a-after-b", "dns_name"),
}
for r in rows:
    print(json.dumps(r))
print("===RESULT===", json.dumps(res))
with open(f"{out}/result.json", "w") as f:
    json.dump(res, f, indent=1)
EOF
