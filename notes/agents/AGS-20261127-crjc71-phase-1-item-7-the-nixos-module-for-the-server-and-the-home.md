---
id: AGS-20261127-crjc71
type: agent_session
schema_version: 1
title: "phase 1 item 7: the NixOS module for the server and the home-manager module for the node, wired into neusis"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T11:37+00:00
finished: 2026-11-27T12:20+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Then start building item 7": phase 1 item 7, the NixOS module for the server and the
home-manager module for the node, with module tests, deployed on ghost001. The owner
asked to build the modules here and wire them into neusis, and will commit, push (comin
deploys) there.

## What happened

- Asked the owner where the delegate key lives on ghost001, what the node module runs,
  and how far to take the deploy. The owner asked whether systemd-creds buys anything over
  agenix: without a TPM it does not (its key sits on the same disk as agenix's host key,
  and it loses the key with the machine). Chosen: agenix, an hourly audit check, modules
  here and wiring in neusis. Recorded as ADR-0016.
- `nitya-server --delegate-key FILE` reads an `age-keygen` identity and makes none of its
  own (1 test).
- `nixosModules.server`: system user `nitya`, state in /var/lib/nitya, the auth key and
  delegate key as credentials, `nitya-admin` through sudo. `homeModules.node`: the
  configured `nitya-node` wrapper and the hourly `audit verify` (systemd timer, launchd
  agent). `checks.modules` evaluates both (the node on Linux and Darwin against stubs),
  reading the scripts' text from their derivations so nothing is built.
- The guide: *The server and node modules* under Deployment; *Running nitya*, the
  deployment overview and *Where it stands* point to it.
- Wired into neusis (its AGS-20261127-cthjec): the server on ghost001 with the leoank key
  and a new `nityaDelegateKey` secret, the node for ank on rogue, with tests. neusis's
  eval suite passes 361/361 against this checkout (run from a copy: its test app reads
  the git checkout, so untracked files and input overrides did not reach it at first).
- Left to the owner: creating the delegate key secret, pushing nitya and updating
  neusis's nitya input, committing and pushing neusis. Item 7 stays open until nitya
  runs on ghost001 and a node enrolls against it.
