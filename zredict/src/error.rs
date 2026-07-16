//! Engine errors. Messages are written in the interface's voice — they say what
//! happened and, where useful, what to do — since they surface directly in the UI.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UserNotFound,
    MarketNotFound,
    /// Action attempted on an already-resolved market.
    MarketResolved,
    /// Predictions attempted after the market's close time.
    PredictionsClosed,
    /// Resolution attempted while a timed market is still accepting predictions.
    TooEarlyToResolve,
    UnknownOutcome,
    ZeroUnits,
    InsufficientBalance { have: u64, need: u64 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UserNotFound => write!(f, "that account doesn't exist"),
            Error::MarketNotFound => write!(f, "that market doesn't exist"),
            Error::MarketResolved => write!(f, "this market is resolved — it's settled and closed"),
            Error::PredictionsClosed => {
                write!(f, "predictions have closed for this market — it's awaiting resolution")
            }
            Error::TooEarlyToResolve => {
                write!(f, "this market is still open for predictions — it can't be resolved yet")
            }
            Error::UnknownOutcome => write!(f, "pick one of the market's listed outcomes"),
            Error::ZeroUnits => write!(f, "stake at least 1 point"),
            Error::InsufficientBalance { have, need } => {
                write!(f, "not enough points: you have {have}, this needs {need}")
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
