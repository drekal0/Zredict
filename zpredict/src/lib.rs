//! zpredict — play-money parimutuel prediction market (Phase 0/1 walking skeleton).
//!
//! This is the thin, end-to-end slice: users hold play-money points, predict an
//! outcome, the pool splits parimutuel-style, and a committee resolves. There is
//! **no privacy layer and no real value** here yet — that is deliberate. The
//! blind-voucher shielding and shielded-ZEC escrow attach later (Phase 2) behind
//! the same seams, and because the parimutuel math is identical, none of this is
//! throwaway.
//!
//! Library surface (storage + money logic) is separated from the HTTP binary so
//! the engine is testable without a running server.

pub mod error;
pub mod models;
pub mod parimutuel;
pub mod store;

pub use error::{Error, Result};
pub use models::*;
pub use store::{MemStore, Repo};
