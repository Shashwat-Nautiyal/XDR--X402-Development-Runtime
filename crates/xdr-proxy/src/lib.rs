use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response, Json},
    routing::{any, get, post},
    Router,
    
};
use reqwest::Client;
use std::net::SocketAddr;
use std::time::Instant;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};
use url::Url;
use xdr_ledger::Ledger;
use xdr_chaos::{ChaosEngine, ChaosConfig};
use serde_json::json; 

// --- Constants ---
const HEADER_UPSTREAM_HOST: &str = "x-upstream-host";
const HEADER_AGENT_ID: &str = "x-agent-id";
const HEADER_SIMULATE_PAYMENT: &str = "x-simulate-payment"; 

// --- State ---
#[derive(Clone)]
struct AppState {
    client: Client,
    ledger: Ledger,
    chaos: ChaosEngine,
}

// --- Classification Enum ---
#[derive(Debug, Clone, PartialEq)]
enum RequestType {
    AiInference,
    Payment,
    Rpc,
    Unknown,
}

#[derive(serde::Deserialize)]
struct BudgetRequest {
    amount: f64,
}

async fn set_agent_budget(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(payload): Json<BudgetRequest>,
) -> impl IntoResponse {
    state.ledger.set_balance(&agent_id, payload.amount);
    info!(target: "xdr_core", "💰 Admin set balance for {} to ${}", agent_id, payload.amount);
    StatusCode::OK
}

pub async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let ledger = Ledger::new();
    let chaos = ChaosEngine::new();
    let state = AppState { client, ledger, chaos };

    let app = Router::new()
        // 1. Management Routes (Internal)
        .route("/_xdr/status/:agent_id", get(get_agent_status))
        .route("/_xdr/budget/:agent_id", post(set_agent_budget))
        .route("/_xdr/chaos", post(update_chaos_config))
        // 2. Proxy Routes (Catch-all)
        .route("/*path", any(proxy_handler)) 
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(target: "xdr_core", "🚀 XDR Proxy listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_agent_status(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    match state.ledger.get_state(&agent_id) {
        Some(agent) => Json(agent).into_response(),
        None => (StatusCode::NOT_FOUND, "Agent not found").into_response(),
    }
}

async fn update_chaos_config(
    State(state): State<AppState>,
    Json(payload): Json<ChaosConfig>,
) -> impl IntoResponse {
    state.chaos.set_config(payload);
    StatusCode::OK
}

async fn proxy_handler(
    State(state): State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let start_time = Instant::now();
    
    // 1. Latency Injection (The Lag)
    if let Some(delay) = state.chaos.inject_latency().await {
        info!(target: "xdr_chaos", "⏳ Injecting Latency: {}ms", delay.as_millis());
        tokio::time::sleep(delay).await;
    }

    // 2. Failure Injection (The Drop)
    if let Some(status_code) = state.chaos.inject_failure() {
        warn!(target: "xdr_chaos", "💥 Injecting Failure: {}", status_code);
        return (
            StatusCode::from_u16(status_code).unwrap(), 
            format!("Chaos Simulation: {}", status_code)
        ).into_response();
    }

    // 1. ENFORCE AGENT IDENTITY
    let agent_id = match req.headers().get(HEADER_AGENT_ID).and_then(|h| h.to_str().ok()) {
        Some(id) => id.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, format!("Missing mandatory header: {}", HEADER_AGENT_ID)).into_response();
        }
    };

    // 2. REGISTER AGENT
    state.ledger.register_or_get(&agent_id);

    // 3. [STAGE 4] x402 PAYMENT SEMANTICS ENGINE
    // Trigger condition: Path contains "paid" OR Header set
    let should_gate = req.uri().path().contains("paid") 
                   || req.headers().contains_key(HEADER_SIMULATE_PAYMENT);

    if should_gate {
        // Check for L402 Token
        let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok());
        
        match auth_header {
            Some(token) if token.starts_with("L402") => {
                // A. PAYMENT ATTEMPT
                let invoice_id = token.replace("L402 ", "");
                match state.ledger.pay_invoice(&invoice_id, &agent_id) {
                    Ok(new_bal) => {
                        info!(target: "xdr_payment", "💰 Payment Verified. Agent: {} Balance: ${:.2}", agent_id, new_bal);
                        // Clean headers so upstream doesn't see our fake token
                        req.headers_mut().remove("Authorization");
                        // Fall through to proxy logic...
                    },
                    Err(e) => {
                       warn!(target: "xdr_payment", "🛑 BLOCKING: Agent {} - {}", agent_id, e);
                        
                        let body = json!({
                            "status": 402,
                            "error": "Payment Failed",
                            "reason": e, // "Wallet Exhausted" or "Safety Limit"
                            "agent": agent_id
                        });
                        return (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
                    }
                }
            },
            _ => {
                // B. PAYMENT REQUIRED (CHALLENGE)
                let amount = 0.01;
                let invoice = state.ledger.create_invoice(&agent_id, amount);
                
                info!(target: "xdr_payment", "🛑 Gating Request. Invoice: {} Cost: ${}", invoice.id, amount);

                let body = json!({
                    "status": 402,
                    "x402_invoice": invoice.id,
                    "amount": format!("{:.2} USDC", amount),
                    "chain": "cronos-testnet"
                });

                let mut resp = Json(body).into_response();
                *resp.status_mut() = StatusCode::PAYMENT_REQUIRED;
                resp.headers_mut().insert(
                    "WWW-Authenticate", 
                    HeaderValue::from_str(&format!("L402 token={}", invoice.id)).unwrap()
                );
                return resp;
            }
        }
    }

    // 4. RESOLVE UPSTREAM URL
    let upstream_url = match resolve_upstream_url(&req) {
        Ok(url) => url,
        Err(err_msg) => return (StatusCode::BAD_REQUEST, err_msg).into_response(),
    };

    // 5. CLASSIFY & LOG
    let req_type = classify_request(&upstream_url, req.method());
    info!(target: "xdr_proxy", "➡️  [{:?}] {} {}", req_type, req.method(), upstream_url);

    // 6. FORWARD UPSTREAM
    // Safety: Strip hop-by-hop headers
    remove_hop_by_hop_headers(req.headers_mut());
    if let Some(host) = upstream_url.host_str() {
        req.headers_mut().insert("host", HeaderValue::from_str(host).unwrap());
    }
    
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    let response = match state.client
        .request(method, upstream_url.clone())
        .headers(headers)
        .body(reqwest::Body::wrap_stream(http_body_util::BodyExt::into_data_stream(body)))
        .send()
        .await 
    {
        Ok(res) => res,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };

    // 7. RETURN RESPONSE
    let status = response.status();
    let mut resp_headers = response.headers().clone();
    remove_hop_by_hop_headers(&mut resp_headers);
    let resp_body = Body::from_stream(response.bytes_stream());

    let mut response_builder = Response::builder().status(status);
    *response_builder.headers_mut().unwrap() = resp_headers;
    response_builder.body(resp_body).unwrap()
}

// --- Helper Logic ---

fn resolve_upstream_url(req: &Request) -> Result<Url, String> {
    let uri = req.uri();

    // Case A: Absolute URL
    if let (Some(_scheme), Some(_host)) = (uri.scheme(), uri.host()) {
        let url_str = uri.to_string();
        return Url::parse(&url_str).map_err(|_| "Invalid Absolute URL".to_string());
    }

    // Case B: Relative URL -> Need Header
    let upstream_host = req.headers()
        .get(HEADER_UPSTREAM_HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing X-Upstream-Host header or Absolute URL")?;

    let path = uri.path();
    let query = uri.query().map(|q| format!("?{}", q)).unwrap_or_default();
    let url_string = format!("https://{}{}{}", upstream_host, path, query);
    
    Url::parse(&url_string).map_err(|_| "Invalid Constructed URL".to_string())
}

fn classify_request(url: &Url, _method: &axum::http::Method) -> RequestType {
    let host = url.host_str().unwrap_or("");
    
    if host.contains("openai.com") || host.contains("anthropic") {
        return RequestType::AiInference;
    }
    if host.contains("cronos") || host.contains("rpc") {
        return RequestType::Rpc;
    }
    
    RequestType::Unknown
}

fn remove_hop_by_hop_headers(headers: &mut HeaderMap) {
    let to_remove = [
        "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
        "te", "trailer", "transfer-encoding", "upgrade", "content-length", 
    ];
    for header in to_remove {
        headers.remove(header);
    }
}