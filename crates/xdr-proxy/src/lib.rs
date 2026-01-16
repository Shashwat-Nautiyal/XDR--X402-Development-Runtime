use axum::{
    body::Body,
    extract::{Path, Request},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use http_body_util::BodyExt;
use reqwest::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, Level};
use url::Url;

// Shared state (client is expensive to create, so we share it)
#[derive(Clone)]
struct AppState {
    client: Client,
}

pub async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .build()?;

    let state = AppState { client };

    // Middleware stack:
    // 1. TraceLayer: Logs every request/response with latency
    let app = Router::new()
        // Catch-all route that captures the entire path
        .route("/*path", any(proxy_handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO).include_headers(true)),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("🚀 XDR Proxy listening on http://{}", addr);
    info!("ℹ️  Usage: http://{}/<TARGET_DOMAIN>/<PATH>", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn proxy_handler(
    state: axum::extract::State<AppState>,
    Path(path_str): Path<String>,
    mut req: Request,
) -> impl IntoResponse {
    let start_time = Instant::now();
    
    // 1. Parse the target: "api.openai.com/v1/chat" -> host="api.openai.com", path="/v1/chat"
    let (target_host, target_path) = match parse_target(&path_str) {
        Some(t) => t,
        None => return (axum::http::StatusCode::BAD_REQUEST, "Invalid Target Format. Use: /<HOST>/<PATH>").into_response(),
    };

    // 2. Construct Upstream URL
    // Assumption: Default to HTTPS. We can add logic to detect http:// later if needed.
    let url_string = format!("https://{}/{}", target_host, target_path);
    let url = match Url::parse(&url_string) {
        Ok(u) => u,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid Upstream URL").into_response(),
    };

    // 3. Prepare the Upstream Request
    // We must strip the 'Host' header, otherwise the upstream server (e.g. OpenAI) 
    // will reject it thinking it was meant for 'localhost'
    req.headers_mut().remove("host");
    
    // Construct reqwest request from axum request
    let method = match reqwest::Method::from_bytes(req.method().as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid HTTP Method").into_response(),
    };
    // Convert axum headers to reqwest headers by rebuilding the HeaderMap
    let mut reqwest_headers = reqwest::header::HeaderMap::new();
    for (name, value) in req.headers() {
        if let (Ok(reqwest_name), Ok(reqwest_value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes())
        ) {
            reqwest_headers.insert(reqwest_name, reqwest_value);
        }
    }
    // Stream the body (zero mutation passthrough)
    let body = req.into_body(); 

    // 4. Send Request
    info!(target: "xdr_proxy", "➡️ Proxying: {} -> {}", method, url);

    let response = match state.client
        .request(method, url)
        .headers(reqwest_headers)
        .body(reqwest::Body::wrap_stream(http_body_util::BodyExt::into_data_stream(body)))
        .send()
        .await 
    {
        Ok(res) => res,
        Err(e) => {
            error!("Upstream error: {}", e);
            return (axum::http::StatusCode::BAD_GATEWAY, format!("Upstream Error: {}", e)).into_response();
        }
    };

    // 5. Construct Downstream Response
    let status = response.status();
    let headers = response.headers().clone();
    let response_body = Body::from_stream(response.bytes_stream());

    let mut response_builder = Response::builder().status(status);
    *response_builder.headers_mut().unwrap() = headers;
    
    let duration = start_time.elapsed();
    info!(target: "xdr_proxy", "⬅️ Response: {} ({}ms)", status, duration.as_millis());

    response_builder.body(response_body).unwrap()
}

// Helper to split "api.openai.com/v1/chat" -> ("api.openai.com", "v1/chat")
fn parse_target(path: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.is_empty() {
        return None;
    }
    let host = parts[0].to_string();
    let rest = if parts.len() > 1 { parts[1] } else { "" };
    Some((host, rest.to_string()))
}