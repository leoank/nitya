# ADR-0011: The API is gRPC; devices enroll with a one-time code and get nitya's own short-lived certificates, bound to their tailnet node id

Status: accepted
Date: 2026-11-26

## Context
ADR-0005 gives every device an Ed25519 key and a short-lived certificate from nitya's
server, bound to its tailnet identity, with signed requests and a deny list. Phase 1
item 2 builds that, and needs what ADR-0005 left open: the API's transport (requests are
signed inside it), how a device first proves it may enroll, the certificate's format, and
what "its tailnet identity" is. The owner chose the first three on 2026-11-26
([[AGS-20261126-hvzeby]]); [[EXP-20261126-hvzed5]] answered the fourth
([[RUN-20261126-hw6442]]): the server reads the caller's stable node id from its own tsnet
status; it survives restarts, a fresh join gets a new one even under the same hostname,
and the node key changes on every restart.

Alternatives considered:
- HTTP and JSON (axum) with HTTP Message Signatures: easier to call by hand, but the
  Nitya workspace lives in saarthi's app (ADR-0002), whose Rust core already speaks tonic
  over tsnet; a second client stack for nothing.
- Approval of a new device from an enrolled one: the first device still needs a code; it
  can come later, as a second way in.
- Trusting tailnet tags or logins: anyone who can add a machine to the tailnet would get
  in, which is what per-device enrollment is meant to prevent.
- X.509 certificates: only nitya verifies them, and WireGuard already encrypts and
  authenticates the transport, so mutual TLS adds nothing; more parsing, more to get wrong.
- Binding the node key or the hostname: the key changes on restart, and the hostname is
  whatever the joining machine asks for.

## Decision
- **gRPC with tonic**, as saarthi does; the proto files live in `crates/proto`
  (`nitya.v1`). The tailnet crate's tonic connector and `Connected` come back from
  saarthi's copy.
- **Enrollment by a one-time code.** An admin command on the server (`nitya-server
  enroll-code --account A --kind node|app|agent`) prints a random 128-bit code, valid
  15 minutes and once; the server keeps only its SHA-256. The device sends the code, its
  Ed25519 public key and a signature over both, over the tailnet. The server checks the
  code and the signature and issues a certificate bound to the node id of the connection.
- **nitya's own certificate**: a protobuf `CertBody` (serial, account, kind, device key,
  tailnet node id, hostname as a label, not-before, not-after) and the CA's Ed25519
  signature over a domain tag and the body's bytes. The CA key is the server's own,
  readable only by the server's user.
- **Lifetime and renewal.** A certificate lives 24 hours. A device renews with a signed
  request; the server renews a certificate up to 30 days past its end if the device is
  not revoked and calls from the same node id. Later than that, the device enrolls again.
- **Signed requests.** Each call carries the certificate, a timestamp, a random nonce and
  the device's signature over a domain tag, the method, the timestamp, the nonce and the
  SHA-256 of the request message as protobuf. The server accepts it when the certificate
  verifies and is current, its device is not revoked, the connection's node id is the
  certificate's, the timestamp is within 60 seconds, the nonce is new within that window,
  and the signature verifies.
- **Revocation** is by device key (`nitya-server revoke`), so a revoked device cannot
  renew under a new serial. Until the store (item 4), the CA's key, issued certificates,
  pending codes and the deny list are files in the server's state directory, where the
  admin commands write them too. Revoking a device from another device waits for Cedar
  (item 5), and rotating the account's keys on revocation for the key hierarchy (item 3,
  ADR-0010).

## Consequences
- A device's tailnet identity and its nitya certificate fail together: a machine whose
  tailnet state is lost rejoins with a new node id and enrolls again.
- The device key is a file in the node's state directory for now (0600); a TPM or the
  Android Keystore, as ADR-0005 wants, is later work.
- The body hash relies on both sides encoding the same message to the same bytes, which
  holds for prost on both ends; a client in another language would sign the bytes it
  sends, and the server would hash the received bytes instead.
- Every authenticated call names an account, a device and a serial, which is what the
  audit log (item 6) records.
