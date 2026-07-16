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
    fn create_market(&self, question: &str, outcomes: Vec<String>, closes_at: Option<u64>) -> Market;
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

    fn create_market(&self, question: &str, outcomes: Vec<String>, closes_at: Option<u64>) -> Market {
        let market = Market {
            id: new_id("m"),
            question: question.trim().to_string(),
            outcomes,
            status: MarketStatus::Open,
            closes_at,
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
        // Order by phase (open, then closed, then resolved), then by question.
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
        let mut units: HashMap<String, u64> = HashMap::new();
        for p in g.positions.iter().filter(|p| p.market_id == market_id) {
            *units.entry(p.outcome.clone()).or_insert(0) += p.units;
        }
        let total: u64 = units.values().sum();
        let outcomes = market
            .outcomes
            .iter()
            .map(|o| {
                let u = units.get(o).copied().unwrap_or(0);
                OutcomeStat {
                    outcome: o.clone(),
                    units: u,
                    implied_prob: if total == 0 { 0.0 } else { u as f64 / total as f64 },
                    payout_multiple: if u == 0 { None } else { Some(total as f64 / u as f64) },
                }
            })
            .collect();
        let phase = phase_of(&market, now());
        Ok(PoolView { market, phase, total_units: total, outcomes })
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

        // Atomic debit + insert (still holding the lock).
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
        // A timed market can only be resolved once its prediction window has closed.
        match phase_of(market, now()) {
            Phase::Resolved => return Err(Error::MarketResolved),
            Phase::Open if market.closes_at.is_some() => return Err(Error::TooEarlyToResolve),
            _ => {} // Closed, or Open with no deadline (committee closes by resolving).
        }
        if !market.outcomes.iter().any(|o| o == winning_outcome) {
            return Err(Error::UnknownOutcome);
        }

        let market_positions: Vec<Position> = g
            .positions
            .iter()
            .filter(|p| p.market_id == market_id)
            .cloned()
            .collect();
        let total_pool: u64 = market_positions.iter().map(|p| p.units).sum();

        let winners: Vec<(String, u64)> = market_positions
            .iter()
            .filter(|p| p.outcome == winning_outcome)
            .map(|p| (p.user_id.clone(), p.units))
            .collect();

        let (winning_units, credits, refunded) = if winners.is_empty() {
            // No one predicted the winning outcome — refund every stake.
            let refunds: Vec<(String, u64)> = market_positions
                .iter()
                .map(|p| (p.user_id.clone(), p.units))
                .collect();
            (0u64, refunds, true)
        } else {
            let (wu, payouts) = parimutuel::payouts(total_pool, &winners);
            (wu, payouts, false)
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
            total_pool,
            winning_units,
            refunded,
            payouts: credits,
        })
    }
}
