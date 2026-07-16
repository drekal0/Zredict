//! Domain models. Play money only — balances and stakes are integer "points",
//! never real ZEC. Positions live server-side; this phase makes NO privacy
//! claims (that arrives with the blind-voucher layer in Phase 2).

use serde::{Deserialize, Serialize};

pub type Id = String;

/// A participant with a play-money balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    pub name: String,
    pub balance: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketStatus {
    Open,
    Resolved,
}

/// A single binary/multi-outcome market.
///
/// Lifecycle: a market accepts predictions while **open**; once `closes_at`
/// passes it is **closed** (no more predictions, awaiting the committee); after
/// the committee acts it is **resolved**. `closes_at` is a unix timestamp in
/// seconds; `None` means "no deadline" (the committee closes it by resolving).
///
/// v0 resolution is a recorded committee action (`resolved_by` + `note` +
/// `resolved_at`); Phase 3 adds a dispute window and multi-sig committee on top
/// of exactly these fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: Id,
    pub question: String,
    pub outcomes: Vec<String>,
    pub status: MarketStatus,
    pub closes_at: Option<u64>,
    pub winning_outcome: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_note: Option<String>,
    pub resolved_at: Option<u64>,
}

/// The lifecycle phase a market is in right now (a function of status + clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Accepting predictions.
    Open,
    /// Deadline passed; predictions stopped; awaiting committee resolution.
    Closed,
    /// Resolved and paid out.
    Resolved,
}

/// Derive the current phase from a market and the current time.
pub fn phase_of(m: &Market, now: u64) -> Phase {
    match m.status {
        MarketStatus::Resolved => Phase::Resolved,
        MarketStatus::Open => match m.closes_at {
            Some(t) if now >= t => Phase::Closed,
            _ => Phase::Open,
        },
    }
}

/// One prediction: `units` points staked on `outcome`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub id: Id,
    pub market_id: Id,
    pub user_id: Id,
    pub outcome: String,
    pub units: u64,
}

/// Public, per-outcome view of a market's pool — the belief-as-a-price signal.
#[derive(Debug, Clone, Serialize)]
pub struct PoolView {
    pub market: Market,
    pub phase: Phase,
    pub total_units: u64,
    /// Per outcome: units staked and implied probability (units / total).
    pub outcomes: Vec<OutcomeStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutcomeStat {
    pub outcome: String,
    pub units: u64,
    pub implied_prob: f64,
    /// Decimal-style payout multiple per unit if this outcome wins
    /// (total_units / units). `None` when nobody has staked it.
    pub payout_multiple: Option<f64>,
}

/// Summary returned when a market resolves.
#[derive(Debug, Clone, Serialize)]
pub struct ResolutionReceipt {
    pub market_id: Id,
    pub winning_outcome: String,
    pub total_pool: u64,
    pub winning_units: u64,
    pub refunded: bool,
    /// (user_id, amount credited)
    pub payouts: Vec<(Id, u64)>,
}
