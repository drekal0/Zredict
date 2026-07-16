//! Lifecycle tests for the play-money engine (storage + parimutuel + market
//! lifecycle), exercised directly against the `Repo` without HTTP.

use zredict::store::now;
use zredict::{Error, MemStore, MarketStatus, Repo};

#[test]
fn full_predict_resolve_payout() {
    let db = MemStore::new();
    let m = db.create_market("Will it rain?", vec!["YES".into(), "NO".into()], None, 0);
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
    let m = db.create_market("Coin flip?", vec!["H".into(), "T".into()], None, 0);
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
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 0);
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
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 0);
    let u = db.create_user("u");
    assert_eq!(db.predict(&m.id, &u.id, "C", 10), Err(Error::UnknownOutcome));
    assert_eq!(db.predict(&m.id, &u.id, "A", 0), Err(Error::ZeroUnits));
}

#[test]
fn two_winners_split_pool_pro_rata() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 0);
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
    let m = db.create_market("q", vec!["A".into(), "B".into()], Some(now() - 1), 0);
    let u = db.create_user("late");
    assert_eq!(db.predict(&m.id, &u.id, "A", 10), Err(Error::PredictionsClosed));
    // ...but a closed market CAN be resolved.
    assert!(db.resolve(&m.id, "A", "committee", "settled").is_ok());
}

#[test]
fn timed_market_cannot_resolve_while_open() {
    let db = MemStore::new();
    // Deadline in the future → still open for predictions.
    let m = db.create_market("q", vec!["A".into(), "B".into()], Some(now() + 3600), 0);
    let u = db.create_user("u");
    // Predictions work while open.
    db.predict(&m.id, &u.id, "A", 10).unwrap();
    // Resolving too early is rejected.
    assert!(matches!(
        db.resolve(&m.id, "A", "committee", ""),
        Err(Error::TooEarlyToResolve)
    ));
}


// ---- seed liquidity ----

#[test]
fn seeded_market_has_a_price_before_any_prediction() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 100);
    let v = db.pool_view(&m.id).unwrap();
    assert_eq!(v.total_units, 200); // 100 seed on each side
    assert_eq!(v.total_real_units, 0);
    for o in &v.outcomes {
        assert_eq!(o.pool_units, 100);
        assert_eq!(o.real_units, 0);
        assert!((o.implied_prob - 0.5).abs() < 1e-9); // opens at 50/50, not empty
        assert!(o.payout_multiple.is_none()); // no real units yet
    }
}

#[test]
fn seed_is_a_bonus_to_the_real_winner() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 100);
    let alice = db.create_user("alice");
    db.predict(&m.id, &alice.id, "A", 100).unwrap();

    // Pool = real(100) + seed(200) = 300; alice is the only real unit on A.
    let r = db.resolve(&m.id, "A", "committee", "").unwrap();
    assert_eq!(r.total_pool, 300);
    assert_eq!(r.seed_subsidy, 200);
    assert_eq!(r.winning_units, 100);
    // Staked 100 (bal 900), scoops the whole 300 -> 1200. The +200 is the seed.
    assert_eq!(db.get_user(&alice.id).unwrap().balance, 1200);
}

#[test]
fn seed_is_forfeited_when_stakes_are_refunded() {
    let db = MemStore::new();
    let m = db.create_market("q", vec!["A".into(), "B".into()], None, 100);
    let alice = db.create_user("alice");
    db.predict(&m.id, &alice.id, "A", 50).unwrap();

    // Nobody predicted B; refund real stakes, seed forfeited (no payout of it).
    let r = db.resolve(&m.id, "B", "committee", "").unwrap();
    assert!(r.refunded);
    assert_eq!(r.seed_subsidy, 0);
    assert_eq!(db.get_user(&alice.id).unwrap().balance, 1000);
}
