-- Turso / libSQL schema for the production swap from the in-memory store.
-- The in-memory `Repo` and this schema model the same data; the only behavioural
-- requirement carried over is ATOMICITY on `predict` and `resolve`.

CREATE TABLE IF NOT EXISTS users (
  id       TEXT PRIMARY KEY,
  name     TEXT NOT NULL,
  balance  INTEGER NOT NULL DEFAULT 1000 CHECK (balance >= 0)
);

CREATE TABLE IF NOT EXISTS markets (
  id               TEXT PRIMARY KEY,
  question         TEXT NOT NULL,
  outcomes         TEXT NOT NULL,           -- JSON array of outcome labels
  status           TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','resolved')),
  winning_outcome  TEXT,
  resolved_by      TEXT,
  resolved_note    TEXT,
  resolved_at      INTEGER
);

CREATE TABLE IF NOT EXISTS positions (
  id         TEXT PRIMARY KEY,
  market_id  TEXT NOT NULL REFERENCES markets(id),
  user_id    TEXT NOT NULL REFERENCES users(id),
  outcome    TEXT NOT NULL,
  units      INTEGER NOT NULL CHECK (units > 0)
);

CREATE INDEX IF NOT EXISTS idx_positions_market ON positions(market_id);
CREATE INDEX IF NOT EXISTS idx_positions_user   ON positions(user_id);

-- Placing a prediction MUST be one transaction. The `AND balance >= :units`
-- guard makes the debit fail (0 rows changed) rather than overspend under a race
-- — the SQL equivalent of holding the store lock across debit + insert:
--
--   BEGIN IMMEDIATE;
--   UPDATE users SET balance = balance - :units
--     WHERE id = :user_id AND balance >= :units;      -- assert changes() == 1
--   INSERT INTO positions (id, market_id, user_id, outcome, units)
--     VALUES (:id, :market_id, :user_id, :outcome, :units);
--   COMMIT;
--
-- Resolving is likewise one transaction: read the pool, credit winners (or refund
-- if no winning units), then flip status to 'resolved'. Only ever resolve a market
-- whose status is still 'open' (assert changes() == 1 on that UPDATE).
