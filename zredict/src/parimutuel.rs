//! Pure parimutuel math — kept separate so the money logic is testable in
//! isolation from storage and HTTP.
//!
//! Parimutuel: all stakes go into one pool; when the outcome is known, the whole
//! pool is split across the winning units in proportion to stake. This is
//! identical whether the units are play-money points or (later) real ZEC — which
//! is exactly why nothing built here is wasted when the money question is decided.

/// Compute payouts for the winning positions given the full pool.
///
/// Returns `(winning_units, payouts)` where `payouts[i]` corresponds to
/// `winning[i]`. Uses `u128` intermediates to avoid overflow, floors each
/// payout (dust from integer division is left in the house — a production
/// system would sweep or redistribute it and is where a fee would attach).
///
/// If there are no winning units, the caller should refund stakes instead
/// (see [`crate::store`]).
pub fn payouts(total_pool: u64, winning: &[(String, u64)]) -> (u64, Vec<(String, u64)>) {
    let winning_units: u64 = winning.iter().map(|(_, u)| *u).sum();
    if winning_units == 0 {
        return (0, Vec::new());
    }
    let out = winning
        .iter()
        .map(|(user, units)| {
            let payout = (*units as u128 * total_pool as u128 / winning_units as u128) as u64;
            (user.clone(), payout)
        })
        .collect();
    (winning_units, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_pool_pro_rata() {
        // Pool of 3 (2 on the winner, 1 on the loser). Winner takes the whole 3.
        let (wu, p) = payouts(3, &[("alice".into(), 2)]);
        assert_eq!(wu, 2);
        assert_eq!(p, vec![("alice".into(), 3)]);
    }

    #[test]
    fn two_winners_share_proportionally() {
        // Pool 100; winners staked 30 and 10 (40 total); losers staked 60.
        // Per-unit payout = 100/40 = 2.5 → 30*2.5=75, 10*2.5=25.
        let (wu, p) = payouts(100, &[("a".into(), 30), ("b".into(), 10)]);
        assert_eq!(wu, 40);
        assert_eq!(p, vec![("a".into(), 75), ("b".into(), 25)]);
    }

    #[test]
    fn no_winners_returns_empty() {
        let (wu, p) = payouts(50, &[]);
        assert_eq!(wu, 0);
        assert!(p.is_empty());
    }
}
