//! Lifecycle tests for the play-money engine (storage + parimutuel + market
//! lifecycle), exercised directly against the `Repo` without HTTP.

use zredict::store::now;
use zredict::{Error, MemStore, MarketStatus, Repo};

#[test]
fn full_predict_resolve_payout() {
    let db = MemStore::new();
    let m = db.create_market("Will it rain?", vec!["YES".into(), "NO".into()], None);
    let alice = db.create_user("alice");
    let bob = db.create_user("bob");
    assert_eq!(alice.balance, 1000);

    db.predict(&m.id, &alice.id, "YES", 100).unwrap();
    db.predict(&m.id, &bob.id, "NO", 100).unwrap();
    assert_eq!(db.pool_view(&m.id).unwrap().total_units, 200);

    let r = db.resolve(&m.id, "YES", "committee", "clear skies were wrong").unwrap();
    assert_eq!(r.total_pool, 200);
    assert_eq!(r.winning_units, 100);
    assert!(!r.refunded);

    assert_eq!(db.get_user(&alice.id).unwrap().balance, 900 + 200);
    assert_eq!(db.get_user(&bob.id).unwrap().balance, 900);

    // Resolved market rejects further predictions.
    assert_eq!(db.predict(&m.id, &bob.id, "NO", 10), Err(Error::MarketResolved));
    assert_eq!(db.pool_view(&m.id).unwrap().market.status, MarketStatus::Resolved);
}

#[test]
fn no_winners_refunds_every_stake() {
    let db = MemStore::new();
    let m = db.create_market("Coin flip?", vec!["H".into(), "T".into()], None);
    let eve = db.create_user("eve");

    db.predict(&m.id, &eve.id, "H", 50).unwrap();
    assert_eq!(db.get_user(&eve.id).unwrap().balance, 950);

    let r = db.resolve(&m.id, "T", "committee", "").unwrap();
    assert!(r.refunded);
    assert_eq!(db.get_user(&eve.id).unwrap().balance, 1000);
}

#[test]
fn cannot_overspend() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None);
    let u = db.create_user("skint");
    assert_eq!(
        db.predict(&m.id, &u.id, "A", 5000),
        Err(Error::InsufficientBalance { have: 1000, need: 5000 })
    );
    assert_eq!(db.get_user(&u.id).unwrap().balance, 1000);
}

#[test]
fn rejects_unknown_outcome_and_zero_stake() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None);
    let u = db.create_user("u");
    assert_eq!(db.predict(&m.id, &u.id, "C", 10), Err(Error::UnknownOutcome));
    assert_eq!(db.predict(&m.id, &u.id, "A", 0), Err(Error::ZeroUnits));
}

#[test]
fn two_winners_split_pool_pro_rata() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None);
    let a = db.create_user("a");
    let b = db.create_user("b");
    let c = db.create_user("c");
    db.predict(&m.id, &a.id, "A", 30).unwrap();
    db.predict(&m.id, &b.id, "A", 10).unwrap();
    db.predict(&m.id, &c.id, "B", 60).unwrap();

    db.resolve(&m.id, "A", "committee", "").unwrap();
    assert_eq!(db.get_user(&a.id).unwrap().balance, 970 + 75);
    assert_eq!(db.get_user(&b.id).unwrap().balance, 990 + 25);
    assert_eq!(db.get_user(&c.id).unwrap().balance, 940);
}

// ---- lifecycle ----

#[test]
fn predictions_rejected_after_close() {
    let db = MemStore::new();
    // Deadline already in the past → market is in the "closed" phase.
    let m = db.create_market("q", vec!["A".into(), "B".into()], Some(now() - 1));
    let u = db.create_user("late");
    assert_eq!(db.predict(&m.id, &u.id, "A", 10), Err(Error::PredictionsClosed));
    // ...but a closed market CAN be resolved.
    assert!(db.resolve(&m.id, "A", "committee", "settled").is_ok());
}

#[test]
fn timed_market_cannot_resolve_while_open() {
    let db = MemStore::new();
    // Deadline in the future → still open for predictions.
    let m = db.create_market("q", vec!["A".into(), "B".into()], Some(now() + 3600));
    let u = db.create_user("u");
    // Predictions work while open.
    db.predict(&m.id, &u.id, "A", 10).unwrap();
    // Resolving too early is rejected.
    assert!(matches!(
        db.resolve(&m.id, "A", "committee", ""),
        Err(Error::TooEarlyToResolve)
    ));
}
