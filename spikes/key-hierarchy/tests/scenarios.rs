//! EXP-20261123-bhfzeh's scenarios, one test each (the plan's step 2), plus the two
//! revocation strategies side by side.

use key_hierarchy::*;

const PASS: &str = "correct horse battery staple";

fn setup() -> (Account, Device, Device, Server) {
    let (laptop, phone) = (Device::new("laptop"), Device::new("phone"));
    register(&laptop);
    register(&phone);
    let mut a = create_account("ank", &laptop, PASS);
    unlock(&mut a, &laptop).unwrap().enroll(&phone);
    (a, laptop, phone, Server::new())
}

/// 1. With a calendar-only delegate the server syncs calendar items while no device is
/// online; mail it receives is sealed to the append key and it cannot read it.
#[test]
fn offline_sync_with_a_calendar_only_delegate() {
    let (mut a, laptop, _phone, mut server) = setup();
    unlock(&mut a, &laptop).unwrap().consent_delegate(Class::Calendar, &server.recipient());
    // every device offline from here
    let cal = server.sync_write(&mut a, Class::Calendar, b"BEGIN:VEVENT standup").unwrap();
    assert_eq!(&**server.read(&a, cal).unwrap(), b"BEGIN:VEVENT standup");
    let mail = server.sync_write(&mut a, Class::Mail, b"From: someone").unwrap();
    assert!(matches!(a.items.iter().find(|i| i.id == mail).unwrap().sealing, Sealing::Append(..)));
    assert!(server.read(&a, mail).is_err(), "the server reads a class it has no delegate for");
    // older mail written by a device, under the mail DEK: unreadable to the server too
    let old = unlock(&mut a, &laptop).unwrap().write(Class::Mail, b"older mail");
    assert!(server.read(&a, old).is_err());
    // the owner comes online: the appended mail is readable and moved under the DEK
    let mut s = unlock(&mut a, &laptop).unwrap();
    assert_eq!(&**s.read(mail).unwrap(), b"From: someone");
    assert_eq!(s.ingest_appended(), 1);
    assert!(matches!(s.account.items.iter().find(|i| i.id == mail).unwrap().sealing, Sealing::Dek(1)));
    // withdrawing consent ends the server's access
    s.withdraw_delegate(Class::Calendar);
    assert!(server.read(&a, cal).is_err());
}

/// 2. Every delegate unwrap and decryption is in the log; tampering is detected.
#[test]
fn every_delegate_use_is_logged_and_tampering_is_detected() {
    let (mut a, laptop, _phone, mut server) = setup();
    unlock(&mut a, &laptop).unwrap().consent_delegate(Class::Calendar, &server.recipient());
    let cal = server.sync_write(&mut a, Class::Calendar, b"event").unwrap();
    server.read(&a, cal).unwrap();
    let mail = server.sync_write(&mut a, Class::Mail, b"mail").unwrap();
    let _ = server.read(&a, mail);
    let actions: Vec<(String, String)> = server.log.entries.iter().map(|e| (e.action.clone(), e.decision.clone())).collect();
    let pairs = |x: &str, d: &str| (x.to_string(), d.to_string());
    assert_eq!(actions, vec![
        pairs("delegate.unwrap", "allow"), pairs("item.encrypt", "allow"),   // sync write, calendar
        pairs("delegate.unwrap", "allow"), pairs("item.decrypt", "allow"),   // agent read, calendar
        pairs("delegate.unwrap", "deny"), pairs("item.append-sealed", "allow"), // sync write, mail
        pairs("item.decrypt", "deny"),                                      // agent read, mail
    ]);
    let pk = server.signer.verifying_key();
    let cp = server.log.checkpoint(); // a device's checkpoint
    server.log.verify(&pk, &[cp]).unwrap();

    let mut edited = server.log.clone();
    edited.entries[3].decision = "deny".into();
    assert!(edited.verify(&pk, &[cp]).unwrap_err().contains("hash"));

    let mut deleted = server.log.clone();
    deleted.entries.remove(2);
    assert!(deleted.verify(&pk, &[cp]).is_err());

    let mut truncated = server.log.clone();
    truncated.entries.truncate(4);
    assert!(truncated.verify(&pk, &[cp]).unwrap_err().contains("shorter"));

    // an admin with the signing key rewrites history and re-signs it all: only a device's
    // checkpoint shows it
    let mut forged = AuditLog::default();
    for e in &server.log.entries {
        let decision = if e.action == "item.decrypt" { "deny" } else { e.decision.as_str() };
        forged.append(&server.signer, &e.actor, &e.action, &e.resource, decision);
    }
    assert!(forged.verify(&pk, &[]).is_ok(), "a re-signed chain is self-consistent");
    assert!(forged.verify(&pk, &[cp]).unwrap_err().contains("rewritten"));

    // a key other than the server's
    let other = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng).verifying_key();
    assert!(server.log.verify(&other, &[]).unwrap_err().contains("signature"));
}

/// 3a. ADR-0005 as written: rotate UK and re-wrap the DEKs only. The revoked device loses
/// its wrap, but with what it kept it still reads data written after its revocation.
#[test]
fn revocation_by_rewrap_only_leaves_new_data_readable_to_the_revoked_device() {
    let (mut a, laptop, phone, server) = setup();
    let kept = Kept { device: Device { id: phone.id.clone(), identity: phone.identity.clone() }, snapshot: a.clone() };
    let before = unlock(&mut a, &laptop).unwrap().write(Class::Mail, b"before");
    unlock(&mut a, &laptop).unwrap().revoke("phone", Revocation::RewrapOnly, PASS, &server.recipient());
    assert!(unlock(&mut a, &phone).is_err(), "the revoked device has no wrap any more");
    let after = unlock(&mut a, &laptop).unwrap().write(Class::Mail, b"after revocation");
    assert_eq!(&**unlock(&mut a, &laptop).unwrap().read(after).unwrap(), b"after revocation");
    // the prediction fails here: the DEK did not change
    assert_eq!(&**kept.try_read(&a, after).unwrap(), b"after revocation");
    assert_eq!(&**kept.try_read(&a, before).unwrap(), b"before");
}

/// 3b. Rotating the DEKs too: new data is out of the revoked device's reach, nothing is
/// lost, old data stays readable to the owner (and to what the device already had).
#[test]
fn revocation_with_dek_rotation_shuts_out_new_data_and_loses_nothing() {
    let (mut a, laptop, phone, mut server) = setup();
    unlock(&mut a, &laptop).unwrap().consent_delegate(Class::Calendar, &server.recipient());
    let kept = Kept { device: Device { id: phone.id.clone(), identity: phone.identity.clone() }, snapshot: a.clone() };
    let before = unlock(&mut a, &laptop).unwrap().write(Class::Mail, b"before");
    unlock(&mut a, &laptop).unwrap().revoke("phone", Revocation::RotateDeks, PASS, &server.recipient());
    let after = unlock(&mut a, &laptop).unwrap().write(Class::Mail, b"after revocation");
    let appended = server.sync_write(&mut a, Class::Mail, b"appended after revocation").unwrap();
    assert!(kept.try_read(&a, after).is_err(), "new data under the new DEK");
    assert!(kept.try_read(&a, appended).is_err(), "new mail sealed to the new append key");
    assert_eq!(&**kept.try_read(&a, before).unwrap(), b"before", "what it could read before, it still can");
    let s = unlock(&mut a, &laptop).unwrap();
    assert_eq!(&**s.read(before).unwrap(), b"before");
    assert_eq!(&**s.read(after).unwrap(), b"after revocation");
    assert_eq!(&**s.read(appended).unwrap(), b"appended after revocation");
    // the delegate keeps working for the consented class under the new DEK version
    let cal = server.sync_write(&mut a, Class::Calendar, b"event after").unwrap();
    assert_eq!(&**server.read(&a, cal).unwrap(), b"event after");
}

/// 5. Every device lost: the recovery passphrase restores UK, the data and a new device.
#[test]
fn recovery_from_the_passphrase_after_every_device_is_lost() {
    let (mut a, laptop, _phone, server) = setup();
    let id = unlock(&mut a, &laptop).unwrap().write(Class::Tasks, b"buy milk");
    unlock(&mut a, &laptop).unwrap().revoke("phone", Revocation::RotateDeks, PASS, &server.recipient());
    // the laptop is lost too: its wrap is deleted
    a.uk_wraps.clear();
    assert!(recover(&mut a, "wrong passphrase").is_err());
    let new = Device::new("new-laptop");
    register(&new);
    let mut s = recover(&mut a, PASS).unwrap();
    assert_eq!(&**s.read(id).unwrap(), b"buy milk");
    s.enroll(&new);
    assert_eq!(&**unlock(&mut a, &new).unwrap().read(id).unwrap(), b"buy milk");
}

/// Items are bound to their account, class and id: moving a ciphertext fails.
#[test]
fn associated_data_binds_items_to_their_place() {
    let (mut a, laptop, _phone, _server) = setup();
    let mut s = unlock(&mut a, &laptop).unwrap();
    let x = s.write(Class::Mail, b"x");
    let y = s.write(Class::Mail, b"y");
    let blob = s.account.items.iter().find(|i| i.id == x).unwrap().blob.clone();
    s.account.items.iter_mut().find(|i| i.id == y).unwrap().blob = blob;
    assert_eq!(s.read(y).unwrap_err(), Error::Crypto);
}
