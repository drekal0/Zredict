//! Storage. A `Repo` trait with an in-memory implementation, so the Turso/libSQL
//! backend swaps in later behind the same interface (schema in `schema.sql`).
//!
//! Every mutating operation takes the store lock for its whole duration, so
//! balance debits and pool updates are atomic — you cannot overspend a balance
//! by racing two predictions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::models::*;
use crate::parimutuel;

const STARTING_BALANCE: u64 = 1_000;

pub trait Repo: Send + Sync {
    fn create_user(&self, name: &str) -> User;
    fn get_user(&self, id: &str) -> Result<User>;
    /// `closes_at`: optional unix-seconds deadline after which predictions stop.
    /// `seed_each`: house subsidy placed on EVERY outcome (0 = no seed).
    fn create_market(
        &self,
        question: &str,
        outcomes: Vec<String>,
        closes_at: Option<u64>,
        seed_each: u64,
    ) -> Market;
    fn list_markets(&self) -> Vec<Market>;
    fn pool_view(&self, market_id: &str) -> Result<PoolView>;
    fn positions_of_user(&self, user_id: &str) -> Vec<Position>;
    fn predict(&self, market_id: &str, user_id: &str, outcome: &str, units: u64) -> Result<Position>;
    fn resolve(
        &self,
        market_id: &str,
        winning_outcome: &str,
        resolved_by: &str,
        note: &str,
    ) -> Result<ResolutionReceipt>;
}

#[derive(Default)]
struct Inner {
    users: HashMap<Id, User>,
    markets: HashMap<Id, Market>,
    positions: Vec<Position>,
}

#[derive(Default)]
pub struct MemStore {
    inner: Mutex<Inner>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

static COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_id(prefix: &str) -> Id {
    format!("{prefix}{:06}", COUNTER.fetch_add(1, Ordering::SeqCst))
}

impl Repo for MemStore {
    fn create_user(&self, name: &str) -> User {
        let user = User {
            id: new_id("u"),
            name: name.trim().to_string(),
            balance: STARTING_BALANCE,
        };
        self.inner.lock().unwrap().users.insert(user.id.clone(), user.clone());
        user
    }

    fn get_user(&self, id: &str) -> Result<User> {
        self.inner
            .lock()
            .unwrap()
            .users
            .get(id)
            .cloned()
            .ok_or(Error::UserNotFound)
    }

    fn create_market(
        &self,
        question: &str,
        outcomes: Vec<String>,
        closes_at: Option<u64>,
        seed_each: u64,
    ) -> Market {
        let seed: HashMap<String, u64> = if seed_each > 0 {
            outcomes.iter().map(|o| (o.clone(), seed_each)).collect()
        } else {
            HashMap::new()
        };
        let market = Market {
            id: new_id("m"),
            question: question.trim().to_string(),
            outcomes,
            status: MarketStatus::Open,
            closes_at,
            seed,
            winning_outcome: None,
            resolved_by: None,
            resolved_note: None,
            resolved_at: None,
        };
        self.inner
            .lock()
            .unwrap()
            .markets
            .insert(market.id.clone(), market.clone());
        market
    }

    fn list_markets(&self) -> Vec<Market> {
        let now = now();
        let mut m: Vec<Market> = self.inner.lock().unwrap().markets.values().cloned().collect();
        m.sort_by(|a, b| {
            let rank = |mk: &Market| match phase_of(mk, now) {
                Phase::Open => 0,
                Phase::Closed => 1,
                Phase::Resolved => 2,
            };
            rank(a).cmp(&rank(b)).then(a.question.cmp(&b.question))
        });
        m
    }

    fn pool_view(&self, market_id: &str) -> Result<PoolView> {
        let g = self.inner.lock().unwrap();
        let market = g.markets.get(market_id).cloned().ok_or(Error::MarketNotFound)?;

        let mut real: HashMap<String, u64> = HashMap::new();
        for p in g.positions.iter().filter(|p| p.market_id == market_id) {
            *real.entry(p.outcome.clone()).or_insert(0) += p.units;
        }
        let total_real: u64 = real.values().sum();
        let total_pool = total_real + market.seed_total();

        let outcomes = market
            .outcomes
            .iter()
            .map(|o| {
                let r = real.get(o).copied().unwrap_or(0);
                let s = market.seed_of(o);
                let pool = r + s;
                OutcomeStat {
                    outcome: o.clone(),
                    real_units: r,
                    seed_units: s,
                    pool_units: pool,
                    implied_prob: if total_pool == 0 { 0.0 } else { pool as f64 / total_pool as f64 },
                    // Return per REAL unit if this wins; seed is a bonus, not a claimant.
                    payout_multiple: if r == 0 { None } else { Some(total_pool as f64 / r as f64) },
                }
            })
            .collect();

        Ok(PoolView {
            phase: phase_of(&market, now()),
            market,
            total_units: total_pool,
            total_real_units: total_real,
            outcomes,
        })
    }

    fn positions_of_user(&self, user_id: &str) -> Vec<Position> {
        self.inner
            .lock()
            .unwrap()
            .positions
            .iter()
            .filter(|p| p.user_id == user_id)
            .cloned()
            .collect()
    }

    fn predict(&self, market_id: &str, user_id: &str, outcome: &str, units: u64) -> Result<Position> {
        if units == 0 {
            return Err(Error::ZeroUnits);
        }
        let mut g = self.inner.lock().unwrap();

        let market = g.markets.get(market_id).ok_or(Error::MarketNotFound)?;
        match phase_of(market, now()) {
            Phase::Resolved => return Err(Error::MarketResolved),
            Phase::Closed => return Err(Error::PredictionsClosed),
            Phase::Open => {}
        }
        if !market.outcomes.iter().any(|o| o == outcome) {
            return Err(Error::UnknownOutcome);
        }

        let user = g.users.get(user_id).ok_or(Error::UserNotFound)?;
        if user.balance < units {
            return Err(Error::InsufficientBalance { have: user.balance, need: units });
        }

        g.users.get_mut(user_id).unwrap().balance -= units;
        let pos = Position {
            id: new_id("p"),
            market_id: market_id.to_string(),
            user_id: user_id.to_string(),
            outcome: outcome.to_string(),
            units,
        };
        g.positions.push(pos.clone());
        Ok(pos)
    }

    fn resolve(
        &self,
        market_id: &str,
        winning_outcome: &str,
        resolved_by: &str,
        note: &str,
    ) -> Result<ResolutionReceipt> {
        let mut g = self.inner.lock().unwrap();

        let market = g.markets.get(market_id).ok_or(Error::MarketNotFound)?;
        match phase_of(market, now()) {
            Phase::Resolved => return Err(Error::MarketResolved),
            Phase::Open if market.closes_at.is_some() => return Err(Error::TooEarlyToResolve),
            _ => {}
        }
        if !market.outcomes.iter().any(|o| o == winning_outcome) {
            return Err(Error::UnknownOutcome);
        }
        let seed_total = market.seed_total();

        let market_positions: Vec<Position> = g
            .positions
            .iter()
            .filter(|p| p.market_id == market_id)
            .cloned()
            .collect();
        let real_total: u64 = market_positions.iter().map(|p| p.units).sum();

        // Real winners only — the house seed never claims.
        let winners: Vec<(String, u64)> = market_positions
            .iter()
            .filter(|p| p.outcome == winning_outcome)
            .map(|p| (p.user_id.clone(), p.units))
            .collect();

        let (winning_units, credits, refunded, paid_pool) = if winners.is_empty() {
            // No real predictor won — refund real stakes; the seed is forfeited.
            let refunds: Vec<(String, u64)> = market_positions
                .iter()
                .map(|p| (p.user_id.clone(), p.units))
                .collect();
            (0u64, refunds, true, real_total)
        } else {
            // Prize pool = real stake + forfeited house seed. Winners split it all.
            let pool = real_total + seed_total;
            let (wu, payouts) = parimutuel::payouts(pool, &winners);
            (wu, payouts, false, pool)
        };

        for (user_id, amount) in &credits {
            if let Some(u) = g.users.get_mut(user_id) {
                u.balance += *amount;
            }
        }

        let m = g.markets.get_mut(market_id).unwrap();
        m.status = MarketStatus::Resolved;
        m.winning_outcome = Some(winning_outcome.to_string());
        m.resolved_by = Some(resolved_by.to_string());
        m.resolved_note = Some(note.to_string());
        m.resolved_at = Some(now());

        Ok(ResolutionReceipt {
            market_id: market_id.to_string(),
            winning_outcome: winning_outcome.to_string(),
            total_pool: paid_pool,
            seed_subsidy: if refunded { 0 } else { seed_total },
            winning_units,
            refunded,
            payouts: credits,
        })
    }
}
