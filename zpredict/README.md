# zpredict — play-money parimutuel prediction market

The **Phase 0/1 walking skeleton**: the thinnest end-to-end slice of the prediction market. Users hold play-money points, predict an outcome, the pool splits parimutuel-style, and a committee resolves. One Rust binary serves both the API and a clickable single-page UI.

**No privacy layer and no real value yet — deliberately.** The blind-voucher shielding and shielded-ZEC escrow attach later (Phase 2) behind the same seams. Because the parimutuel math is identical whether the units are points or ZEC, none of this is throwaway.

## Run it

```bash
cargo run          # -> http://localhost:3000
cargo test         # 8 tests: parimutuel math + full lifecycle
```

Open the page, pick a name (1,000 free points), and predict. To create or resolve markets, open the **Committee panel** and enter the dev admin token `committee-dev-token`.

## What it proves

- End-to-end lifecycle over a real HTTP API: create market → predict → resolve → payout.
- **Parimutuel payouts** (winners split the whole pool pro-rata; refunds if nobody picked the winner).
- **Atomic balances** — you can't overspend by racing two predictions (the debit + insert happen under one lock; the SQL equivalent is in `schema.sql`).
- **Committee resolution as a recorded action** (`resolved_by` + `note` + timestamp) — the honest v0 of the oracle. The hard problem to sweat next is making this *feel* legitimate; the data model is already shaped for the Phase 3 dispute window + multi-sig committee.

## Shape

```
src/
  models.rs      User / Market / Position / PoolView
  parimutuel.rs  pure payout math (unit-tested in isolation)
  store.rs       Repo trait + atomic in-memory MemStore
  error.rs       engine errors, written in the interface's voice
  main.rs        Axum server + JSON API + serves the UI
static/
  index.html     single-page UI; the pool-split bar is the crowd's belief, priced
schema.sql       Turso/libSQL schema for the production swap
tests/
  lifecycle.rs   predict/resolve/payout/refund/overspend/closed-market
```

## API

| Method | Path | Notes |
|--------|------|-------|
| POST | `/api/users` | `{name}` → new user with 1,000 points |
| GET | `/api/users/:id` | user + their positions |
| GET | `/api/markets` | all markets with pool views |
| GET | `/api/markets/:id` | one market's pool view |
| POST | `/api/markets` | **admin** · `{question, outcomes[]}` |
| POST | `/api/markets/:id/predict` | `{user_id, outcome, units}` |
| POST | `/api/markets/:id/resolve` | **admin** · `{winning_outcome, note}` |

Admin routes require the `x-admin-token` header. This is a **placeholder** for the committee — see the Phase 3 note above.

## Where this sits in the plan

- **v0/v1 (this repo):** play money, positions server-side, no privacy claims.
- **Phase 2:** swap `MemStore` → Turso (`schema.sql`); add shielded-ZEC escrow via NEAR Intents; add the blind-voucher layer (`rsa_blind_issuer`) so predictions become unlinkable to wallets.
- **Phase 3+:** resolution dispute window, multi-sig committee, on-chain shielded pool.

## Notes

The frontend is intentionally a zero-build single file so the skeleton runs with one command and exercises the exact API the Next.js 14 app will call — porting it is a straight swap of the same `fetch` calls. Front-end framework boilerplate is not where the first slice should spend its budget.
