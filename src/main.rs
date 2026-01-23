use clap::{Parser, Subcommand};
use anyhow::Result;
use serde_json::json;
use xdr_chaos::ChaosConfig;
use xdr_trace::Trace;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

// 1. CLI Definition
#[derive(Parser)]
#[command(name = "xdr")]
#[command(about = "x402 Dev Runtime - The Foundry for AI Agents", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Sets the port for the XDR Proxy
    #[arg(short, long, default_value_t = 4002, global = true)]
    port: u16,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the XDR runtime server
    Run{
        /// Select Network Environment
        #[arg(long, default_value = "cronos-testnet")]
        network: String,
    },
    /// Manage Chaos engineering settings
    Chaos {
        #[command(subcommand)]
        action: ChaosAction,
    },
    /// Show current status of the runtime
    Status{
        /// The Agent ID to query
        #[arg(short, long)]
        agent: String,
    },
    Budget {
        #[arg(short, long)]
        agent: String,
        #[arg(long)]
        set: f64,
    },
    Logs {
        /// Filter by Agent ID
        #[arg(short, long)]
        agent: Option<String>,
        
        /// Output Raw JSON
        #[arg(long)]
        json: bool,
    }
}

#[derive(Subcommand)]
enum ChaosAction {
    Disable,
    /// Enable chaos with specific parameters
    Enable {
        /// RNG Seed for determinism
        #[arg(long, default_value_t = 42)]
        seed: u64,
        
        /// Rate of 5xx/429 errors (0.0 - 1.0)
        #[arg(long, default_value_t = 0.0)]
        failure_rate: f64,
        
        /// Rate of Payment Rejections (0.0 - 1.0)
        #[arg(long, default_value_t = 0.0)]
        payment_failure: f64,

        /// Rate of "Rug Pulls" (0.0 - 1.0)
        #[arg(long, default_value_t = 0.0)]
        rug_rate: f64,

        #[arg(long, default_value_t = 0)]
        min_latency: u64,
        
        #[arg(long, default_value_t = 0)]
        max_latency: u64,
    },
}

// 2. Main Entry Point
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 4. Command Router
    match &cli.command {
        Commands::Run{network} => {
            // NOTE: No tracing subscriber when running TUI - it corrupts the display
            // Tracing is only used for non-TUI commands
            
            // 1. Create Shared State (owned by main, shared with proxy and TUI)
            let ledger = xdr_ledger::Ledger::new();
            let chaos = xdr_chaos::ChaosEngine::new();
            let traces: Arc<Mutex<VecDeque<Trace>>> = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));

            // 2. Clone for Proxy (runs in background task)
            let proxy_ledger = ledger.clone();
            let proxy_chaos = chaos.clone();
            let proxy_traces = traces.clone();
            let proxy_network = network.clone();
            let proxy_port = cli.port;

            // 3. Spawn Proxy in Background Task
            tokio::spawn(async move {
                if let Err(e) = xdr_proxy::run_server(
                    proxy_port, 
                    proxy_network, 
                    proxy_ledger, 
                    proxy_chaos, 
                    proxy_traces
                ).await {
                    eprintln!("Proxy crashed: {}", e);
                }
            });

            // 4. Run TUI in Foreground (Main Thread)
            // Brief delay to let proxy bind to port
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            let tui_app = xdr_tui::App {
                ledger,
                chaos,
                traces,
            };

            if let Err(e) = xdr_tui::run_tui(tui_app).await {
                 eprintln!("TUI Error: {}", e);
            }
            
            // When TUI quits (user hits 'q'), the program exits
            println!("Shutting down XDR...");
        }
        Commands::Status { agent } => {
            let url = format!("http://localhost:{}/_xdr/status/{}", cli.port, agent);
            match reqwest::get(&url).await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        println!("{}", body);
                    } else {
                        // NEW: Print the actual status code (e.g., 404 or 400)
                        eprintln!("❌ Error [{}]: Agent '{}' not found.", resp.status(), agent);
                    }
                }
                Err(e) => eprintln!("❌ Connection failed: {}", e),
            }
        }
        Commands::Budget { agent, set } => {
            let client = reqwest::Client::new();
            let url = format!("http://localhost:{}/_xdr/budget/{}", cli.port, agent);
            
            let res = client.post(&url)
                .json(&json!({ "amount": set }))
                .send()
                .await;

            match res {
                Ok(r) if r.status().is_success() => println!("✅ Budget updated for {}", agent),
                Ok(r) => eprintln!("❌ Failed: {}", r.status()),
                Err(e) => eprintln!("❌ Connection failed: {}", e),
            }
        }
        Commands::Chaos { action } => {
            let config = match action {
                ChaosAction::Disable => ChaosConfig::default(),
                ChaosAction::Enable { seed, failure_rate, payment_failure, rug_rate, min_latency, max_latency } => ChaosConfig {
                    enabled: true,
                    seed: *seed,
                    global_failure_rate: *failure_rate,
                    payment_failure_rate: *payment_failure,
                    rug_rate: *rug_rate,
                    min_latency_ms: *min_latency,
                    max_latency_ms: *max_latency,
                },
            };

            let client = reqwest::Client::new();
            let url = format!("http://localhost:{}/_xdr/chaos", cli.port);
            
            match client.post(&url).json(&config).send().await {
                Ok(r) if r.status().is_success() => println!("🌪️ Chaos configuration updated."),
                Ok(r) => eprintln!("❌ Server error: {}", r.status()),
                Err(e) => eprintln!("❌ Connection failed: {}", e),
            }
        }
        Commands::Logs { agent, json } => {
             let url = format!("http://localhost:{}/_xdr/traces", cli.port);
             match reqwest::get(&url).await {
                Ok(res) => {
                    let traces: Vec<Trace> = res.json().await.unwrap_or_default();
                    
                    for trace in traces {
                        // Filter
                        if let Some(ref a) = agent {
                            if &trace.agent_id != a { continue; }
                        }
                        
                        if *json {
                            println!("{}", serde_json::to_string(&trace).unwrap());
                        } else {
                            // Human Readable Format
                            println!("------------------------------------------------");
                            println!("🆔 [{}] {} {}", trace.status_code.unwrap_or(0), trace.method, trace.url);
                            println!("   Agent: {} | Duration: {}ms", trace.agent_id, trace.duration_ms.unwrap_or(0));
                            for event in trace.events {
                                println!("   - [{:?}] {}", event.category, event.message);
                            }
                        }
                    }
                },
                Err(_) => eprintln!("❌ Could not fetch logs"),
             }
        }
    }

    Ok(())
}