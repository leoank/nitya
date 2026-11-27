# ADR-0013: An account's keys are one key set its devices write whole; a new device gets them only by approval on a device, by fingerprint

Status: accepted (2026-11-27; the owner chose approval by fingerprint, rotation by the next device to unlock, and a required recovery passphrase; the rest settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
ADR-0005 and ADR-0010 give each account a user key (UK) wrapped to its devices and to a
recovery passphrase, data keys (DEKs) per class and version under UK, an append key, and
DEKs wrapped to a sync delegate by consent; [[EXP-20261123-bhfzeh]] built it in a spike.
Building it into the server leaves open what the spike did not have to decide: the
server never holds UK, yet enrolls devices, revokes them, and stores the keys. The
owner's choice of 2026-11-26 adds a separate X25519 wrap key per device, beside its
Ed25519 signing key. Asked on 2026-11-27 ([[AGS-20261127-1c5mft]]), the owner chose:
- a new device gets UK only when an unlocked device approves it, after the person
  compares its fingerprint, not automatically at enrollment;
- after a revocation the account is flagged and the next device to unlock rotates; sync
  goes on meanwhile;
- the recovery passphrase is required when the first keys are made.

Alternatives considered:
- Wrapping UK to every newly enrolled device automatically: the admin issues enrollment
  codes, so the admin could enroll a device of their own and read the account.
- Pausing delegated sync until a device rotates: nothing new exposed, but sync waits on
  the owner being online; the exposure in the gap needs the revoked device's kept keys
  and the server's database together.
- Tables per wrap kind in the store: more schema for data the server cannot interpret; a
  device must write all of it at once on rotation anyway.

## Decision
- **The key set** is one protobuf message per account (`nitya.v1.KeySet`), stored in the
  store's `key_sets` table with a revision. Devices read it (`Keys/Get`), change it with
  UK in memory, and write it back whole (`Keys/Put`) if it is still at the revision they
  read. Both calls are signed (ADR-0011).
- **Wrap keys.** Each device has an age X25519 identity (`wrap.key`), its recipient in
  the enrollment request, the enrollment proof and the certificate (`wrap_key`). UK is
  wrapped to it with age.
- **Approval.** `nitya-node keys approve SERIAL FPR` on an unlocked device wraps UK to
  device SERIAL only if its wrap key's fingerprint (80 bits of SHA-256, shown by
  `keys fingerprint` on the new device) is FPR. Each device wrap carries a tag under UK
  over its signing and wrap keys; a device checks every tag when it unlocks, so a wrap
  key the server adds or swaps is caught.
- **Recovery** is an age X25519 key whose secret is sealed under Argon2id (64 MiB, 3
  passes) of the passphrase; UK is wrapped to its public half, with a tag under UK. A
  rotation re-wraps the new UK to it without the passphrase. `keys recover` on an
  enrolled device unlocks with the passphrase and approves that device.
- **Revocation** (ADR-0010): `nitya-server revoke` deletes the device's wrap from the key
  set and flags the account. The server then refuses any write that does not rotate: a
  higher UK version, a new DEK version for every class and a new append key. The first
  device to unlock rotates on its own before doing anything else.
- **The server checks** what it can without UK: a write keeps every DEK and append key
  version, keeps the recovery key, wraps only to unrevoked devices of the account, and
  has a DEK for each class (mail, calendar, tasks, tokens, files).
- **The sync delegate** is an age X25519 key in `delegate.key` beside the store, until
  the NixOS module passes it as a systemd credential (item 7). `keys consent CLASS`
  wraps the class's current DEK to it; `keys withdraw CLASS` deletes every version's
  wrap. With consent the server seals and opens the class's items; without, it seals new
  items to the append key and cannot open them.
- **Items** are `nitya.v1.SealedItem`: the key version, an item key wrapped to the append
  key when the server sealed it without consent, and XChaCha20-Poly1305 ciphertext whose
  associated data binds the account, class, item and version.

## Consequences
- An admin cannot give themselves an account's keys through enrollment, nor by editing
  the key set; they can withhold or delete wraps, which locks devices out until a
  recovery, but reads nothing.
- Approving a device needs the person at two devices, or the fingerprint carried between
  them; the app's flow (later) shows it as a code or QR.
- Until a device unlocks after a revocation, items are still written under the old keys;
  a revoked device that kept them and also gets the database reads those.
- The delegate's unwraps are not yet in the audit log; ADR-0007's log is item 6.
- The recovery key's secret never changes, so a revoked device that ever knew the
  passphrase can, with a copy of the database, still recover; changing the passphrase is
  a later command.
