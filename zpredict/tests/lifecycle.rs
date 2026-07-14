//! Lifecycle tests for the play-money engine (storage + parimutuel), exercised
//! directly against the `Repo` without HTTP.

use zpredict::{Error, MemStore, MarketStatus, Repo};

#[test]
fn full_predict_resolve_payout() {
    let db = MemStore::new();
    let m = db.create_market("Will it rain?", vec!["YES".into(), "NO".into()]);
    let alice = db.create_user("alice");
    let bob = db.create_user("bob");
    assert_eq!(alice.balance, 1000);

    // Alice stakes 100 on YES, Bob stakes 100 on NO. Pool = 200.
    db.predict(&m.id, &alice.id, "YES", 100).unwrap();
    db.predict(&m.id, &bob.id, "NO", 100).unwrap();

    let view = db.pool_view(&m.id).unwrap();
    assert_eq!(view.total_units, 200);

    // Resolve YES: Alice (the only YES unit) takes the whole pool.
    let r = db.resolve(&m.id, "YES", "committee", "clear skies were wrong").unwrap();
    assert_eq!(r.total_pool, 200);
    assert_eq!(r.winning_units, 100);
    assert!(!r.refunded);

    assert_eq!(db.get_user(&alice.id).unwrap().balance, 900 + 200); // staked 100, won 200
    assert_eq!(db.get_user(&bob.id).unwrap().balance, 900); // staked 100, lost

    // Market is closed to further predictions.
    assert_eq!(
        db.predict(&m.id, &bob.id, "NO", 10),
        Err(Error::MarketClosed)
    );
    assert_eq!(db.pool_view(&m.id).unwrap().market.status, MarketStatus::Resolved);
}

#[test]
fn no_winners_refunds_every_stake() {
    let db = MemStore::new();
    let m = db.create_market("Coin flip?", vec!["H".into(), "T".into()]);
    let eve = db.create_user("eve");

    db.predict(&m.id, &eve.id, "H", 50).unwrap();
    assert_eq!(db.get_user(&eve.id).unwrap().balance, 950);

    // Nobody predicted T; resolving T refunds all stakes.
    let r = db.resolve(&m.id, "T", "committee", "").unwrap();
    assert!(r.refunded);
    assert_eq!(db.get_user(&eve.id).unwrap().balance, 1000);
}

#[test]
fn cannot_overspend() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()]);
    let u = db.create_user("skint");
    assert_eq!(
        db.predict(&m.id, &u.id, "A", 5000),
        Err(Error::InsufficientBalance { have: 1000, need: 5000 })
    );
    // Failed prediction must not have moved the balance.
    assert_eq!(db.get_user(&u.id).unwrap().balance, 1000);
}

#[test]
fn rejects_unknown_outcome_and_zero_stake() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()]);
    let u = db.create_user("u");
    assert_eq!(db.predict(&m.id, &u.id, "C", 10), Err(Error::UnknownOutcome));
    assert_eq!(db.predict(&m.id, &u.id, "A", 0), Err(Error::ZeroUnits));
}

#[test]
fn two_winners_split_pool_pro_rata() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()]);
    let a = db.create_user("a");
    let b = db.create_user("b");
    let c = db.create_user("c");
    // A-side: a=30, b=10 (40 units). B-side: c=60. Pool=100.
    db.predict(&m.id, &a.id, "A", 30).unwrap();
    db.predict(&m.id, &b.id, "A", 10).unwrap();
    db.predict(&m.id, &c.id, "B", 60).unwrap();

    db.resolve(&m.id, "A", "committee", "").unwrap();
    // per-unit = 100/40 = 2.5 → a:75, b:25
    assert_eq!(db.get_user(&a.id).unwrap().balance, 970 + 75);
    assert_eq!(db.get_user(&b.id).unwrap().balance, 990 + 25);
    assert_eq!(db.get_user(&c.id).unwrap().balance, 940); // lost 60
}
