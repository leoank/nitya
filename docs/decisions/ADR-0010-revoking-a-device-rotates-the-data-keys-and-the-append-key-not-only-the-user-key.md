# ADR-0010: Revoking a device rotates the data keys and the append key, not only the user key

Status: proposed
Date: 2026-11-26

## Context
ADR-0005 revokes a device by deleting its wrap and rotating the user key (UK), "only DEKs
are re-wrapped". [[EXP-20261123-bhfzeh]] built the hierarchy and showed that this does
not revoke ([[RUN-20261126-fexqm6]]). The DEKs stay the same, and an enrolled device can
read the account's wraps (they are on the server, and it holds UK). A device that kept
them, or the DEKs themselves, still opens every item written after its revocation. The
same holds for the append key that seals new items of classes without a delegate.

Alternatives considered: re-encrypting every item under new DEKs at revocation (costs
time per item, and gives nothing: the device could already read the old items); relying
on devices to forget keys (cannot be checked, and a stolen device is the case that
matters).

## Decision
Revoking a device, besides deleting its wrap and rotating UK:
- gives each data class a new DEK version, wrapped by the new UK, which every write uses
  from then on; earlier versions stay, re-wrapped by the new UK, to read older items;
- renews the sync-delegate wrap of each consented class with the new DEK version;
- makes a new append key pair, its private half under the new UK, for items sealed from
  then on.

Items are not re-encrypted. Every item names its key version (ADR-0005's "versioned
keys") and binds it in its associated data. The rest of ADR-0005 stands.

## Consequences
- A revoked device reads nothing written after its revocation; what it could read before,
  it can still read if it kept it. That is the limit of any revocation without
  re-encryption, and is said so in the device list.
- Revocation stays cheap: ~70 ms for one account in the spike, nearly all of it the
  Argon2id re-wrap of the recovery key, independent of the number of items.
- Accounts accumulate DEK versions, one per revocation; a later background job may
  re-encrypt old items and drop old versions if that ever matters.
- When accepted, ADR-0005 is marked as amended by this record.
