//! HTTP server for the play-money prediction market.
//!
//! One binary: serves the single-page UI at `/` and a small JSON API at `/api/*`.
//! State is an `Arc<dyn Repo>`, so swapping the in-memory store for a Turso-backed
//! one is a one-line change here.
//!
//! Admin actions (create market, resolve) are gated on an `x-admin-token` header.
//! This is a PLACEHOLDER for the committee — Phase 3 replaces it with a real
//! multi-sig committee and a dispute window.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use zpredict::{MemStore, Repo};

type Db = Arc<dyn Repo>;

const ADMIN_TOKEN: &str = "committee-dev-token"; // placeholder; see module docs
const INDEX_HTML: &str = include_str!("../static/index.html");

#[tokio::main]
async fn main() {
    let db: Db = Arc::new(MemStore::new());
    seed(&db); // a couple of demo markets so the page isn't empty

    let app = Router::new()
        .route("/", get(index))
        .route("/api/users", post(create_user))
        .route("/api/users/:id", get(get_user))
        .route("/api/markets", get(list_markets).post(create_market))
        .route("/api/markets/:id", get(get_market))
        .route("/api/markets/:id/predict", post(predict))
        .route("/api/markets/:id/resolve", post(resolve))
        .with_state(db);

    let addr = "0.0.0.0:3000";
    println!("zpredict listening on http://localhost:3000");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn seed(db: &Db) {
    db.create_market(
        "Will ZEC shielded-pool share exceed 35% by end of 2026?",
        vec!["YES".into(), "NO".into()],
    );
    db.create_market(
        "Will Ztarknet ship a public mainnet before 2027?",
        vec!["YES".into(), "NO".into()],
    );
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

// ---- request bodies ----

#[derive(Deserialize)]
struct NewUser {
    name: String,
}

#[derive(Deserialize)]
struct NewMarket {
    question: String,
    outcomes: Vec<String>,
}

#[derive(Deserialize)]
struct PredictReq {
    user_id: String,
    outcome: String,
    units: u64,
}

#[derive(Deserialize)]
struct ResolveReq {
    winning_outcome: String,
    #[serde(default)]
    resolved_by: String,
    #[serde(default)]
    note: String,
}

// ---- handlers ----

async fn create_user(State(db): State<Db>, Json(b): Json<NewUser>) -> Result<Response, AppError> {
    let name = if b.name.trim().is_empty() { "anon".to_string() } else { b.name };
    Ok(Json(db.create_user(&name)).into_response())
}

async fn get_user(State(db): State<Db>, Path(id): Path<String>) -> Result<Response, AppError> {
    let user = db.get_user(&id)?;
    let positions = db.positions_of_user(&id);
    Ok(Json(json!({ "user": user, "positions": positions })).into_response())
}

async fn list_markets(State(db): State<Db>) -> Result<Response, AppError> {
    let views: Vec<_> = db
        .list_markets()
        .into_iter()
        .filter_map(|m| db.pool_view(&m.id).ok())
        .collect();
    Ok(Json(views).into_response())
}

async fn get_market(State(db): State<Db>, Path(id): Path<String>) -> Result<Response, AppError> {
    Ok(Json(db.pool_view(&id)?).into_response())
}

async fn create_market(
    State(db): State<Db>,
    headers: HeaderMap,
    Json(b): Json<NewMarket>,
) -> Result<Response, AppError> {
    require_admin(&headers)?;
    let outcomes: Vec<String> = b
        .outcomes
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if b.question.trim().is_empty() || outcomes.len() < 2 {
        return Err(AppError(StatusCode::BAD_REQUEST,
            "a market needs a question and at least two outcomes".into()));
    }
    Ok(Json(db.create_market(&b.question, outcomes)).into_response())
}

async fn predict(
    State(db): State<Db>,
    Path(id): Path<String>,
    Json(b): Json<PredictReq>,
) -> Result<Response, AppError> {
    let pos = db.predict(&id, &b.user_id, &b.outcome, b.units)?;
    let pool = db.pool_view(&id)?;
    let user = db.get_user(&b.user_id)?;
    Ok(Json(json!({ "position": pos, "pool": pool, "user": user })).into_response())
}

async fn resolve(
    State(db): State<Db>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(b): Json<ResolveReq>,
) -> Result<Response, AppError> {
    require_admin(&headers)?;
    let by = if b.resolved_by.trim().is_empty() { "committee".into() } else { b.resolved_by };
    let receipt = db.resolve(&id, &b.winning_outcome, &by, &b.note)?;
    Ok(Json(receipt).into_response())
}

// ---- admin + errors ----

fn require_admin(headers: &HeaderMap) -> Result<(), AppError> {
    let ok = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == ADMIN_TOKEN)
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(AppError(StatusCode::UNAUTHORIZED,
            "committee action: provide the admin token".into()))
    }
}

struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<zpredict::Error> for AppError {
    fn from(e: zpredict::Error) -> Self {
        use zpredict::Error::*;
        let code = match e {
            UserNotFound | MarketNotFound => StatusCode::NOT_FOUND,
            MarketClosed | UnknownOutcome | ZeroUnits | InsufficientBalance { .. } => {
                StatusCode::BAD_REQUEST
            }
        };
        AppError(code, e.to_string())
    }
}
