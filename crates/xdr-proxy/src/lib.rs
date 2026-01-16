use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use reqwest::Client;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Instant;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};
use url::Url;

// --- Constants ---
const HEADER_UPSTREAM_HOST: &str = "x-upstream-host";
const HEADER_AGENT_ID: &str = "x-agent-id";

// --- State ---
#[derive(Clone)]
struct AppState {
    client: Client,
}

// --- Classification Enum (The Hook) ---
#[derive(Debug, Clone, PartialEq)]
enum RequestType {
    AiInference,
    Payment,
    Rpc,
    Unknown,
}

pub async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        // Disable automatic redirect following to act as a true proxy
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let state = AppState { client };

    let app = Router::new()
        // Catch-all handler
        .route("/*path", any(proxy_handler)) 
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(target: "xdr_core", "🚀 XDR Proxy listening on http://{}", addr);
    info!(target: "xdr_core", "ℹ️  Config: Set X-Upstream-Host or use Absolute URLs");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn proxy_handler(
    state: axum::extract::State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let start_time = Instant::now();
    
    // 1. EXTRACT AGENT IDENTITY (Optional for now, but logged)
    let agent_id = req.headers()
        .get(HEADER_AGENT_ID)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    // 2. RESOLVE UPSTREAM URL
    // Priority 1: Absolute URL (e.g. from curl proxy or RPC)
    // Priority 2: X-Upstream-Host header + Request Path
    let upstream_url = match resolve_upstream_url(&req) {
        Ok(url) => url,
        Err(err_msg) => {
            warn!(target: "xdr_proxy", "Resolution failed: {}", err_msg);
            return (StatusCode::BAD_REQUEST, err_msg).into_response();
        }
    };

    // 3. CLASSIFY REQUEST (The Hook)
    // Currently a no-op, but ready for Stage 3 logic
    let req_type = classify_request(&upstream_url, req.method());
    
    info!(
        target: "xdr_proxy", 
        "➡️  [{}] {} {} (Agent: {})", 
        format!("{:?}", req_type).to_uppercase(), 
        req.method(), 
        upstream_url,
        agent_id
    );

    // 4. PREPARE REQUEST
    // Strip hop-by-hop headers that might confuse the upstream
    remove_hop_by_hop_headers(req.headers_mut());
    // Ensure Host header matches the upstream, not localhost
    if let Some(host) = upstream_url.host_str() {
        req.headers_mut().insert("host", HeaderValue::from_str(host).unwrap());
    }
    
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    // 5. FORWARD UPSTREAM
    let response = match state.client
        .request(method, upstream_url.clone())
        .headers(headers)
        .body(reqwest::Body::wrap_stream(http_body_util::BodyExt::into_data_stream(body)))
        .send()
        .await 
    {
        Ok(res) => res,
        Err(e) => {
            error!(target: "xdr_proxy", "Upstream error: {}", e);
            return (StatusCode::BAD_GATEWAY, format!("Upstream Error: {}", e)).into_response();
        }
    };

    // 6. PROCESS RESPONSE
    let status = response.status();
    let mut resp_headers = response.headers().clone();
    
    // Safety: Remove hop-by-hop headers from UPSTREAM response before sending to DOWNSTREAM client
    remove_hop_by_hop_headers(&mut resp_headers);

    let resp_body = Body::from_stream(response.bytes_stream());

    let mut response_builder = Response::builder().status(status);
    *response_builder.headers_mut().unwrap() = resp_headers;
    
    info!(
        target: "xdr_proxy", 
        "⬅️  [{}] {} ({}ms)", 
        status,
        upstream_url, 
        start_time.elapsed().as_millis()
    );

    response_builder.body(resp_body).unwrap()
}

// --- Helper Logic ---

fn resolve_upstream_url(req: &Request) -> Result<Url, String> {
    let uri = req.uri();

    // Case A: Absolute URL (e.g., "https://api.openai.com/v1/chat")
    if let (Some(scheme), Some(host)) = (uri.scheme(), uri.host()) {
        let url_str = uri.to_string();
        return Url::parse(&url_str).map_err(|_| "Invalid Absolute URL".to_string());
    }

    // Case B: Relative URL -> Need Header
    let upstream_host = req.headers()
        .get(HEADER_UPSTREAM_HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing X-Upstream-Host header or Absolute URL")?;

    // Construct: https:// + {Header} + {Path} + {Query}
    let path = uri.path();
    let query = uri.query().map(|q| format!("?{}", q)).unwrap_or_default();
    
    // Default to HTTPS. A production tool might check for X-Forwarded-Proto
    let url_string = format!("https://{}{}{}", upstream_host, path, query);
    
    Url::parse(&url_string).map_err(|_| "Invalid Constructed URL".to_string())
}

// Placeholder for Stage 3 Logic
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
    // Standard hop-by-hop headers that must be dropped by proxies
    let to_remove = [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "content-length", // We are streaming, so length might change or be chunked
    ];

    for header in to_remove {
        headers.remove(header);
    }
}