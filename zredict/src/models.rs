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

/// A single binary/multi-outcome market. v0 resolution is a recorded committee
/// action (`resolved_by` + `note` + `resolved_at`); Phase 3 adds a dispute
/// window and multi-sig committee on top of exactly these fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: Id,
    pub question: String,
    pub outcomes: Vec<String>,
    pub status: MarketStatus,
    pub winning_outcome: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_note: Option<String>,
    pub resolved_at: Option<u64>,
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
