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
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::time::Instant;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};
use url::Url;
use xdr_ledger::Ledger;
use xdr_chaos::{ChaosEngine, ChaosConfig};
use xdr_trace::{Trace, EventCategory};
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
    traces: Arc<Mutex<VecDeque<Trace>>>,
    network: String,
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

pub async fn run_server(port: u16, network:String) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let ledger = Ledger::new();
    let chaos = ChaosEngine::new();
    let traces = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
    let state = AppState { client, ledger, chaos, traces, network: network.clone(), };

    let app = Router::new()
        // 1. Management Routes (Internal)
        .route("/_xdr/status/:agent_id", get(get_agent_status))
        .route("/_xdr/budget/:agent_id", post(set_agent_budget))
        .route("/_xdr/chaos", post(update_chaos_config))
        .route("/_xdr/traces", get(get_traces))
        // 2. Proxy Routes (Catch-all)
        .route("/*path", any(proxy_handler)) 
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(target: "xdr_core", "🌍 Network Mode: {} (Chain ID: 338)", network);

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

async fn get_traces(State(state): State<AppState>) -> impl IntoResponse {
    let traces = state.traces.lock().unwrap();
    // Return the list (JSON)
    Json(traces.clone()).into_response()
}

async fn proxy_handler(
    State(state): State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let start_time = Instant::now();
    
    

    // 1. ENFORCE AGENT IDENTITY
    let agent_id = match req.headers().get(HEADER_AGENT_ID).and_then(|h| h.to_str().ok()) {
        Some(id) => id.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, format!("Missing mandatory header: {}", HEADER_AGENT_ID)).into_response();
        }
    };

    // 2. REGISTER AGENT
    state.ledger.register_or_get(&agent_id);

    // 3. Latency Injection (The Lag)
    // if let Some(delay) = state.chaos.inject_latency().await {
    //     info!(target: "xdr_chaos", "⏳ Injecting Latency: {}ms", delay.as_millis());
    //     tokio::time::sleep(delay).await;
    // }
    state.chaos.inject_latency().await;

    // 4. Failure Injection (The Drop)
    // if let Some(status_code) = state.chaos.inject_failure() {
    //     warn!(target: "xdr_chaos", "💥 Injecting Failure: {}", status_code);
    //     return (
    //         StatusCode::from_u16(status_code).unwrap(), 
    //         format!("Chaos Simulation: {}", status_code)
    //     ).into_response();
    // }
    if let Some(status_code) = state.chaos.roll_network_failure() {
        warn!(target: "xdr_chaos", "💥 Network Failure Injected: {}", status_code);
        return (StatusCode::from_u16(status_code).unwrap(), "Chaos: Network Error").into_response();
    }

    let mut trace = Trace::new("unknown", req.method().as_str(), &req.uri().to_string());
    
    // Helper macro to save typing
    macro_rules! record {
        ($cat:expr, $msg:expr) => { trace.log($cat, &$msg) };
    }

    // 1. CHAOS (Latency)
    state.chaos.inject_latency().await;
    
    // 2. CHAOS (Network Failure)
    if let Some(status_code) = state.chaos.roll_network_failure() {
        record!(EventCategory::Chaos, format!("Injected Network Failure: {}", status_code));
        trace.finish(status_code);
        state.traces.lock().unwrap().push_back(trace); // Commit trace
        return (StatusCode::from_u16(status_code).unwrap(), "Chaos Error").into_response();
    }

    // 3. IDENTITY
    let agent_id = match req.headers().get(HEADER_AGENT_ID).and_then(|h| h.to_str().ok()) {
        Some(id) => id.to_string(),
        None => {
            record!(EventCategory::Error, "Missing X-Agent-ID header".to_string());
            trace.finish(400);
            state.traces.lock().unwrap().push_back(trace);
            return (StatusCode::BAD_REQUEST, "Missing X-Agent-ID").into_response();
        }
    };
    trace.agent_id = agent_id.clone(); // Update correct ID
    record!(EventCategory::Info, format!("Agent identified: {}", agent_id));

    // 4. REGISTER
    state.ledger.register_or_get(&agent_id);

    // 5. PAYMENT LOGIC
    let should_gate = req.uri().path().contains("paid") 
                   || req.headers().contains_key(HEADER_SIMULATE_PAYMENT);

    if should_gate {
        let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok());
        match auth_header {
            Some(token) if token.starts_with("L402") => {
                // Payment Chaos
                if state.chaos.roll_payment_failure() {
                    record!(EventCategory::Chaos, "Payment transaction failed on-chain".to_string());
                    trace.finish(402);
                    state.traces.lock().unwrap().push_back(trace);
                    return (StatusCode::PAYMENT_REQUIRED, "Chaos: Payment Failed").into_response();
                }

                let invoice_id = token.replace("L402 ", "");
               match state.ledger.pay_invoice(&invoice_id, &agent_id, &state.network) {
                    Ok(receipt) => {
                        // LOG THE CRONOS DATA
                        record!(EventCategory::Payment, format!(
                            "Payment Confirmed on Cronos (Testnet). Tx: {} | Block: {}", 
                            receipt.tx_hash, receipt.block_height
                        ));
                        
                        // Trace the economics
                        record!(EventCategory::Info, format!(
                            "Wallet: {:.2} USDC | Chain: {}", 
                            receipt.new_balance, receipt.chain_id
                        ));
                        record!(EventCategory::Payment, format!("Payment accepted. Bal: ${:.2}", receipt.new_balance));
                        
                        // Rug Chaos
                        if state.chaos.roll_rug_pull() {
                             record!(EventCategory::Chaos, "RUG PULL: Payment taken, request dropped".to_string());
                             trace.finish(500);
                             state.traces.lock().unwrap().push_back(trace);
                             return (StatusCode::INTERNAL_SERVER_ERROR, "Rug Pull").into_response();
                        }
                        
                        req.headers_mut().remove("Authorization");
                    },
                    Err(e) => {
                        record!(EventCategory::Payment, format!("Payment rejected: {}", e));
                        trace.finish(402);
                        state.traces.lock().unwrap().push_back(trace);
                        
                        // Copy the specific budget error logic from Stage 5 here
                        let body = json!({ "status": 402, "error": e, "agent": agent_id });
                        return (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
                    }
                }
            },
            _ => {
                // Generate Invoice
                let invoice = state.ledger.create_invoice(&agent_id, 0.01);
                record!(EventCategory::Payment, format!("Generated Invoice: {}", invoice.id));
                trace.finish(402);
                state.traces.lock().unwrap().push_back(trace);
                
                // Copy the L402 response logic here
                let body = json!({
                    "status": 402,
                    "x402_invoice": invoice.id,
                    "amount": "0.01",
                    "currency": "USDC",
                    "chain": "cronos",
                    "network": state.network,
                    "chain_id": 338, // Cronos Testnet ID
                    "payment_address": "0x000000000000000000000000000000000000dead" // Burn addr for mock
                });
                
                let mut resp = Json(body).into_response();
                *resp.status_mut() = StatusCode::PAYMENT_REQUIRED;
                resp.headers_mut().insert("WWW-Authenticate", HeaderValue::from_str(&format!("L402 token={}", invoice.id)).unwrap());
                return resp;
            }
        }
    }

    // 6. UPSTREAM
    let upstream_url = match resolve_upstream_url(&req) {
        Ok(u) => u,
        Err(e) => {
            record!(EventCategory::Error, format!("Resolution failed: {}", e));
            trace.finish(400);
            state.traces.lock().unwrap().push_back(trace);
            return (StatusCode::BAD_REQUEST, e).into_response();
        }
    };
    
    record!(EventCategory::Upstream, format!("Forwarding to {}", upstream_url));

    // 7. CLASSIFY & LOG
    let req_type = classify_request(&upstream_url, req.method());
    info!(target: "xdr_proxy", "➡️  [{:?}] {} {}", req_type, req.method(), upstream_url);

    // 8. FORWARD UPSTREAM
    // Safety: Strip hop-by-hop headers
    remove_hop_by_hop_headers(req.headers_mut());
    if let Some(host) = upstream_url.host_str() {
        req.headers_mut().insert("host", HeaderValue::from_str(host).unwrap());
    }
    
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    let response = match state.client.request(method, upstream_url).headers(headers).body(reqwest::Body::wrap_stream(http_body_util::BodyExt::into_data_stream(body))).send().await {
        Ok(res) => res,
        Err(e) => {
            record!(EventCategory::Upstream, format!("Upstream Failed: {}", e));
            trace.finish(502);
            state.traces.lock().unwrap().push_back(trace);
            return (StatusCode::BAD_GATEWAY, e.to_string()).into_response();
        }
    };

    // 9. RETURN RESPONSE
    let status = response.status();
    record!(EventCategory::Upstream, format!("Upstream responded: {}", status));
    trace.finish(status.as_u16());

    {
        let mut store = state.traces.lock().unwrap();
        if store.len() >= 1000 { store.pop_front(); } // Ring buffer logic
        store.push_back(trace);
    }

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