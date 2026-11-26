//! Timings for EXP-20261123-bhfzeh (the plan's step 3): item encryption and decryption
//! over 100k synthetic items, revocation with each strategy on an account holding them,
//! one delegate unwrap, and the Argon2id recovery unwrap. Prints one `===RESULT===` line.
//!
//!   key-hierarchy [items] [item-bytes]      (defaults 100000 and 2048)

use std::time::Instant;

use key_hierarchy::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let pass = "correct horse battery staple";
    let laptop = Device::new("laptop");
    let phone = Device::new("phone");
    register(&laptop);
    register(&phone);
    let server = Server::new();

    let t = Instant::now();
    let mut a = create_account("ank", &laptop, pass);
    let create_ms = t.elapsed().as_secs_f64() * 1e3;
    unlock(&mut a, &laptop).unwrap().enroll(&phone);
    unlock(&mut a, &laptop).unwrap().consent_delegate(Class::Calendar, &server.recipient());

    let payload = vec![b'x'; size];
    let mut s = unlock(&mut a, &laptop).unwrap();
    let t = Instant::now();
    for _ in 0..n {
        s.write(Class::Calendar, &payload);
    }
    let enc_us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    // decryption with the DEK in hand (what a device or the unlocked delegate does per item)
    let dek = s.dek(Class::Calendar, 1).unwrap();
    let t = Instant::now();
    let mut total = 0usize;
    for it in &s.account.items {
        let aad = format!("nitya/v1|ank|item|Calendar|{}|dek1", it.id);
        total += open(&dek, &it.blob, aad.as_bytes()).unwrap().len();
    }
    let dec_us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    assert_eq!(total, n as usize * size);
    drop(s);

    let mut b = a.clone();
    let t = Instant::now();
    unlock(&mut a, &laptop).unwrap().revoke("phone", Revocation::RewrapOnly, pass, &server.recipient());
    let rewrap_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    unlock(&mut b, &laptop).unwrap().revoke("phone", Revocation::RotateDeks, pass, &server.recipient());
    let rotate_ms = t.elapsed().as_secs_f64() * 1e3;

    let w = b.delegate_wraps.values().next().unwrap().clone();
    let t = Instant::now();
    let _ = unwrap_with(&server.delegate, &w).unwrap();
    let delegate_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let _ = recover(&mut b, pass).unwrap();
    let recovery_ms = t.elapsed().as_secs_f64() * 1e3;

    println!(
        "===RESULT=== {{\"items\": {n}, \"item_bytes\": {size}, \"encrypt_us_per_item\": {enc_us:.2}, \"decrypt_us_per_item\": {dec_us:.2}, \"revoke_rewrap_only_ms\": {rewrap_ms:.1}, \"revoke_rotate_deks_ms\": {rotate_ms:.1}, \"delegate_unwrap_ms\": {delegate_ms:.2}, \"recovery_unwrap_ms\": {recovery_ms:.1}, \"create_account_ms\": {create_ms:.1}}}"
    );
}
