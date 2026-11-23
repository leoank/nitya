# ADR-0004: Agents get scoped grants, draft rather than send, and act through approved plans; nitya spawns saarthi sessions first

Status: accepted
Date: 2026-11-23

## Context
nitya should give agents enough context to automate mundane tasks, through saarthi, which
already spawns Claude Code sessions on any node from its control service. msgvault
(@msgvault) gives agents delegated grants per source that never imply each other, never
sends mail, and makes calendar writes dry-run plans re-checked before execution. Inbox
Zero (@inbox-zero) dropped its approval queue for automatic rules: approval that adds
friction gets abandoned.

Alternatives considered: automatic rules whitelisted to run without approval (deferred: the
owner chose the stricter baseline); MCP for any session first (chosen second); only
spawned sessions, never MCP (too narrow for sessions the owner starts by hand).

## Decision
- An agent acts under a grant: a set of actions, each separate and none implying another
  (`calendar.read`, `calendar.event.read`, `calendar.write`, `calendar.invite`,
  `mail.search`, `mail.read`, `draft.create`, `task.write`, ...), scoped to sources and
  time, issued per job and revocable.
- nitya never sends mail on an agent's behalf by default; agents create drafts.
- A write is a plan first: the agent proposes, the owner approves (one tap, batched, from
  the Nitya workspace, ADR-0002), nitya re-checks access and executes; an unknown outcome
  is reported, never replayed. Every step is in the audit log.
- First, nitya calls saarthi's control service to spawn sessions for its own jobs
  (triage, scheduling) holding a narrowed nitya grant; next, the nitya node serves an MCP
  server so any session on that machine can use nitya under the machine's grants.

## Consequences
- nitya is a client of saarthi's control service, with a client token of its own.
- The approval path must be fast enough to be used; measure it in
  [[EXP-20261123-bhfz7e]].
