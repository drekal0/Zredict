//! Domain models. Play money only — balances and stakes are integer "points",
//! never real ZEC. Positions live server-side; this phase makes NO privacy
//! claims (that arrives with the blind-voucher layer in Phase 2).

use std::collections::HashMap;

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
/// Lifecycle: accepts predictions while **open**; once `closes_at` passes it is
/// **closed** (awaiting the committee); after the committee acts it is
/// **resolved**. `closes_at` is unix seconds; `None` means "no deadline".
///
/// `seed` is an optional house subsidy per outcome. It gives a fresh market a
/// starting price (so the bar isn't empty) and is added to the prize pool, but
/// the house never claims a payout — the seed is forfeited at resolution and
/// becomes a bonus split among the real winners. It never competes with them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: Id,
    pub question: String,
    pub outcomes: Vec<String>,
    pub status: MarketStatus,
    pub closes_at: Option<u64>,
    /// House seed units per outcome (outcome -> units). Absent/zero = no seed.
    pub seed: HashMap<String, u64>,
    pub winning_outcome: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_note: Option<String>,
    pub resolved_at: Option<u64>,
}

impl Market {
    pub fn seed_total(&self) -> u64 {
        self.seed.values().sum()
    }
    pub fn seed_of(&self, outcome: &str) -> u64 {
        self.seed.get(outcome).copied().unwrap_or(0)
    }
}

/// The lifecycle phase a market is in right now (a function of status + clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Open,
    Closed,
    Resolved,
}

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
    /// Total pool driving the price = real stake + house seed.
    pub total_units: u64,
    /// Real predictor stake only (excludes seed) — the basis for payouts.
    pub total_real_units: u64,
    pub outcomes: Vec<OutcomeStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutcomeStat {
    pub outcome: String,
    /// Real predictor units on this outcome.
    pub real_units: u64,
    /// House seed units on this outcome.
    pub seed_units: u64,
    /// real + seed — what drives the bar and the price.
    pub pool_units: u64,
    /// Implied probability = pool_units / total pool.
    pub implied_prob: f64,
    /// Indicative return per REAL unit if this outcome wins
    /// (total pool / real_units). `None` when no real units back it yet.
    pub payout_multiple: Option<f64>,
}

/// Summary returned when a market resolves.
#[derive(Debug, Clone, Serialize)]
pub struct ResolutionReceipt {
    pub market_id: Id,
    pub winning_outcome: String,
    /// Full prize pool paid out = real stake + forfeited house seed.
    pub total_pool: u64,
    /// The portion that was house subsidy.
    pub seed_subsidy: u64,
    pub winning_units: u64,
    pub refunded: bool,
    /// (user_id, amount credited)
    pub payouts: Vec<(Id, u64)>,
}
