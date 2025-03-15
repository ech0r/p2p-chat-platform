use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info};
use turn_server::{TurnConfig, TurnServerManager};
use web_server::{WebServerConfig, WebServerManager};

#[derive(Parser, Debug)]
#[clap(
    name = "p2p-chat-server",
    about = "P2P Chat Server with integrated TURN server",
    version
)]
struct Cli {
    /// IP address to advertise for TURN server
    #[clap(long, env = "TURN_PUBLIC_IP", default_value = "127.0.0.1")]
    turn_public_ip: IpAddr,
    
    /// Port for the TURN server
    #[clap(long, env = "TURN_PORT", default_value = "3478")]
    turn_port: u16,
    
    /// Authentication realm for TURN server
    #[clap(long, env = "TURN_REALM", default_value = "coyote.technology")]
    turn_realm: String,
    
    /// TURN username
    #[clap(long, env = "TURN_USERNAME", default_value = "p2pchat")]
    turn_username: String,
    
    /// TURN password
    #[clap(long, env = "TURN_PASSWORD", default_value = "p2pchat-password")]
    turn_password: String,
    
    /// IP address to bind the web server to
    #[clap(long, env = "WEB_BIND_IP", default_value = "0.0.0.0")]
    web_bind_ip: IpAddr,
    
    /// Port for the web server
    #[clap(long, env = "WEB_PORT", default_value = "8080")]
    web_port: u16,
    
    /// Path to static files directory (if not provided, embedded assets will be used)
    #[clap(long, env = "STATIC_DIR")]
    static_dir: Option<PathBuf>,
}

fn verify_embedded_assets() {
    // Check for essential files
    let index_html = Asset::get("index.html");
    let js_file = Asset::get("assets/p2p_chat_wasm.js");
    let wasm_file = Asset::get("assets/p2p_chat_wasm_bg.wasm");

    let mut files: Vec<_> = Asset::iter().collect();
    files.sort();
    for file in &files {
        let content = Asset::get(file).unwrap();
        println!(" - {} ({} bytes)", file, content.data.len());
    }
    
    if index_html.is_none() {
        tracing::warn!("Embedded index.html not found!");
    } else {
        tracing::info!("Found embedded index.html ({} bytes)", index_html.unwrap().data.len());
    }
    
    if js_file.is_none() {
        tracing::warn!("Embedded JS file not found!");
    } else {
        tracing::info!("Found embedded JS file ({} bytes)", js_file.unwrap().data.len());
    }
    
    if wasm_file.is_none() {
        tracing::warn!("Embedded WASM file not found!");
    } else {
        tracing::info!("Found embedded WASM file ({} bytes)", wasm_file.unwrap().data.len());
    }
}

// Use rust-embed to embed the static assets in the binary
#[derive(rust_embed::RustEmbed)]
#[folder = "static/"]
struct Asset;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Verify embedded assets
    verify_embedded_assets();

    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Load environment variables from .env file if present
    dotenv::dotenv().ok();
    
    // Parse command-line arguments
    let args = Cli::parse();
    
    // Configure the TURN server
    let turn_config = TurnConfig {
        public_ip: args.turn_public_ip,
        port: args.turn_port,
        realm: args.turn_realm,
        users: vec![(args.turn_username, args.turn_password)],
    };
    
    // Create a TURN server manager
    let mut turn_manager = TurnServerManager::new(turn_config.clone());
    
    // Get TURN server connection details
    let turn_details = turn_manager.get_connection_details();
    
    // Configure the web server
    let web_config = WebServerConfig {
        bind_ip: args.web_bind_ip,
        port: args.web_port,
        static_dir: args.static_dir,  // This can be None to use embedded assets
        turn_details: Some(turn_details),
    };
    
    // Create a web server manager
    let mut web_manager = WebServerManager::new(web_config);
    
    // Create a channel to signal shutdown
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    
    // Start the TURN server in a separate task
    let turn_handle: JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
        if let Err(e) = turn_manager.start().await {
            error!("TURN server error: {}", e);
            return Err(anyhow::anyhow!("TURN server error: {}", e));
        }
        Ok(())
    });
    
    // Start the web server in a separate task
    let web_handle: JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
        if let Err(e) = web_manager.start().await {
            error!("Web server error: {}", e);
            return Err(anyhow::anyhow!("Web server error: {}", e));
        }
        Ok(())
    });
    
    // Set up signal handling for graceful shutdown
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal");
                let _ = shutdown_tx_clone.send(()).await;
            }
            Err(e) => {
                error!("Failed to listen for Ctrl+C: {}", e);
            }
        }
    });
    
    // Wait for shutdown signal
    shutdown_rx.recv().await;
    info!("Shutting down...");
    
    // Abort the server tasks
    turn_handle.abort();
    web_handle.abort();
    
    info!("Servers stopped, goodbye!");
    
    Ok(())
}
