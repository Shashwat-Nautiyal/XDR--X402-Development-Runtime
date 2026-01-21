use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use anyhow::Result;
use serde_json::json;
use xdr_chaos::ChaosConfig;

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
    Run,
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
}

#[derive(Subcommand)]
enum ChaosAction {
    Enable,
    Disable,
}

// 2. Main Entry Point
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 3. Initialize Structured Logging (JSON)
    let subscriber = FmtSubscriber::builder()
        // Use JSON formatting for machine-readability (good for future TUI)
        .json() 
        .with_max_level(if cli.verbose { Level::DEBUG } else { Level::INFO })
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    // 4. Command Router
    match &cli.command {
        Commands::Run => {
            info!(
                target: "xdr_core",
                event = "startup",
                port = cli.port,
                msg = "Starting XDR Runtime"
            );
            
            // Delegate to the xdr-proxy crate
            if let Err(e) = xdr_proxy::run_server(cli.port).await {
                tracing::error!("Server crashed: {}", e);
                std::process::exit(1);
            }
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
                ChaosAction::Enable => ChaosConfig {
                    enabled: true,
                    failure_rate: 0.2,   // 20%
                    min_latency_ms: 500,
                    max_latency_ms: 1500,
                },
                ChaosAction::Disable => ChaosConfig::default(), // enabled: false
            };

            let client = reqwest::Client::new();
            let url = format!("http://localhost:{}/_xdr/chaos", cli.port);
            
            match client.post(&url).json(&config).send().await {
                Ok(r) if r.status().is_success() => println!("🌪️ Chaos configuration updated."),
                Ok(r) => eprintln!("❌ Server error: {}", r.status()),
                Err(e) => eprintln!("❌ Connection failed: {}", e),
            }
        }
    }

    Ok(())
}