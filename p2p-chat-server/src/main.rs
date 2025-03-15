use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;

use clap::Parser;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
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
    
    /// Enable debug output
    #[clap(long, env = "DEBUG", action = clap::ArgAction::SetTrue)]
    debug: bool,
}

// Use rust-embed to embed the static assets in the binary
#[derive(rust_embed::RustEmbed)]
#[folder = "static/"]
#[include = "*.html"]
#[include = "assets/*"]
struct Asset;

fn verify_embedded_assets() -> bool {
    // Check for essential files
    let index_html = Asset::get("index.html");
    let js_file = Asset::get("assets/p2p_chat_wasm.js");
    let wasm_file = Asset::get("assets/p2p_chat_wasm_bg.wasm");

    info!("Embedded assets found:");
    let mut files: Vec<_> = Asset::iter().collect();
    files.sort();
    
    for file in files {
        let content = Asset::get(&file).unwrap();
        info!(" - {} ({} bytes)", file, content.data.len());
    }
    
    let mut all_files_present = true;
    
    if index_html.is_none() {
        warn!("Embedded index.html not found!");
        all_files_present = false;
    } else {
        info!("Found embedded index.html ({} bytes)", index_html.unwrap().data.len());
    }
    
    if js_file.is_none() {
        warn!("Embedded JS file not found!");
        all_files_present = false;
    } else {
        info!("Found embedded JS file ({} bytes)", js_file.unwrap().data.len());
    }
    
    if wasm_file.is_none() {
        warn!("Embedded WASM file not found!");
        all_files_present = false;
    } else {
        info!("Found embedded WASM file ({} bytes)", wasm_file.unwrap().data.len());
    }
    
    all_files_present
}

fn check_static_files(static_dir: &Option<PathBuf>) -> bool {
    if let Some(dir) = static_dir {
        info!("Checking static files in {}", dir.display());
        
        let index_path = dir.join("index.html");
        let assets_dir = dir.join("assets");
        let js_path = assets_dir.join("p2p_chat_wasm.js");
        let wasm_path = assets_dir.join("p2p_chat_wasm_bg.wasm");
        
        let mut all_files_present = true;
        
        if !index_path.exists() {
            warn!("Static index.html not found at {}", index_path.display());
            all_files_present = false;
        } else {
            if let Ok(meta) = fs::metadata(&index_path) {
                info!("Found static index.html ({} bytes)", meta.len());
            }
        }
        
        if !js_path.exists() {
            warn!("Static JS file not found at {}", js_path.display());
            all_files_present = false;
        } else {
            if let Ok(meta) = fs::metadata(&js_path) {
                info!("Found static JS file ({} bytes)", meta.len());
            }
        }
        
        if !wasm_path.exists() {
            warn!("Static WASM file not found at {}", wasm_path.display());
            all_files_present = false;
        } else {
            if let Ok(meta) = fs::metadata(&wasm_path) {
                info!("Found static WASM file ({} bytes)", meta.len());
            }
        }
        
        return all_files_present;
    }
    
    // No static directory specified
    false
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command-line arguments
    let args = Cli::parse();
    
    // Initialize tracing with the appropriate level
    let filter_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("p2p_chat_server={},turn_server={},web_server={}", filter_level, filter_level, filter_level))
        .init();
    
    // Load environment variables from .env file if present
    dotenv::dotenv().ok();
    
    // Check static files or embedded assets
    let using_static_files = check_static_files(&args.static_dir);
    let using_embedded_assets = verify_embedded_assets();
    
    if !using_static_files && !using_embedded_assets {
        error!("Neither static files nor embedded assets are available!");
        error!("Please run 'make build-wasm' to compile the WebAssembly module or specify a valid static directory.");
        return Err(anyhow::anyhow!("No static assets available"));
    }
    
    // Configure the TURN server
    let turn_config = TurnConfig {
        public_ip: args.turn_public_ip,
        port: args.turn_port,
        realm: args.turn_realm.clone(),
        users: vec![(args.turn_username.clone(), args.turn_password.clone())],
    };
    
    // Create a TURN server manager
    let mut turn_manager = TurnServerManager::new(turn_config.clone());
    
    // Get TURN server connection details
    let turn_details = turn_manager.get_connection_details();
    
    // Print server details
    info!("TURN server details:");
    info!("  Public IP: {}", args.turn_public_ip);
    info!("  Port: {}", args.turn_port);
    info!("  Realm: {}", args.turn_realm);
    info!("  Username: {}", args.turn_username);
    info!("  Password: {}", if args.turn_password.len() > 0 { "****" } else { "empty" });
    
    // Configure the web server
    let web_config = WebServerConfig {
        bind_ip: args.web_bind_ip,
        port: args.web_port,
        static_dir: args.static_dir.clone(),  // This can be None to use embedded assets
        turn_details: Some(turn_details),
    };
    
    info!("Web server details:");
    info!("  Bind IP: {}", args.web_bind_ip);
    info!("  Port: {}", args.web_port);
    if let Some(path) = &args.static_dir {
        info!("  Static files directory: {}", path.display());
    } else {
        info!("  Using embedded static files");
    }
    
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
    
    // Print service URLs
    info!("");
    info!("======================================================");
    info!("Server started successfully!");
    info!("Web interface: http://{}:{}", args.web_bind_ip, args.web_port);
    info!("TURN server: {}:{}", args.turn_public_ip, args.turn_port);
    info!("======================================================");
    info!("");
    info!("Press Ctrl+C to stop the server");
    
    // Wait for shutdown signal
    shutdown_rx.recv().await;
    info!("Shutting down...");
    
    // Abort the server tasks
    turn_handle.abort();
    web_handle.abort();
    
    info!("Servers stopped, goodbye!");
    
    Ok(())
}
