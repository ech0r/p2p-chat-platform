use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, get_service},
    Json, Router,
};
use http::header;
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{debug, error, info};
use turn_server::TurnConnectionDetails;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "static/"]
struct Asset;

#[derive(Debug, Error)]
pub enum WebServerError {
    #[error("Web server error: {0}")]
    Server(#[from] anyhow::Error),
    
    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, WebServerError>;

/// Configuration for the web server
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    /// IP address to bind the server to
    pub bind_ip: IpAddr,
    
    /// Port to bind the server to
    pub port: u16,
    
    /// Path to the static files directory
    pub static_dir: Option<PathBuf>,
    
    /// TURN server connection details
    pub turn_details: Option<TurnConnectionDetails>,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            bind_ip: "0.0.0.0".parse().unwrap(),
            port: 8080,
            static_dir: None,
            turn_details: None,
        }
    }
}

/// Web server state
#[derive(Debug, Clone)]
struct AppState {
    turn_details: Option<TurnConnectionDetails>,
}

/// Response with embedded assets
pub enum AssetResponse {
    NotFound,
    Asset {
        content: Vec<u8>,
        content_type: String,
    },
}

impl IntoResponse for AssetResponse {
    fn into_response(self) -> Response {
        match self {
            AssetResponse::NotFound => StatusCode::NOT_FOUND.into_response(),
            AssetResponse::Asset { content, content_type } => {
                let headers = [(header::CONTENT_TYPE, content_type)];
                (headers, content).into_response()
            }
        }
    }
}

/// Web server manager
pub struct WebServerManager {
    config: WebServerConfig,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl WebServerManager {
    /// Create a new web server manager with the given configuration
    pub fn new(config: WebServerConfig) -> Self {
        Self {
            config,
            shutdown_tx: None,
        }
    }
    
    /// Start the web server
    pub async fn start(&mut self) -> Result<()> {
        // Create app state
        let state = Arc::new(AppState {
            turn_details: self.config.turn_details.clone(),
        });
        
        // Setup CORS layer
        let cors = CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_origin(Any);
        
        // Create router based on presence of static files directory
        let router = if let Some(static_dir) = &self.config.static_dir {
            info!("Serving static files from: {:?}", static_dir);
            
            // Create a service to serve static files
            let serve_dir = ServeDir::new(static_dir);
            
            Router::new()
                .route("/api/turn-config", get(get_turn_config))
                .nest_service("/", get_service(serve_dir).handle_error(|err| async move {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Static file error: {}", err),
                    )
                }))
                .layer(cors)
                .layer(TraceLayer::new_for_http())
                .with_state(state)
        } else {
            info!("Using embedded assets");
            
            Router::new()
                .route("/", get(serve_index))
                .route("/index.html", get(serve_index))
                .route("/api/turn-config", get(get_turn_config))
                .route("/assets/*path", get(serve_embedded_asset))
                .layer(cors)
                .layer(TraceLayer::new_for_http())
                .with_state(state)
        };
        
        // Create server address
        let addr = SocketAddr::new(self.config.bind_ip, self.config.port);
        
        info!("Starting web server on {}...", addr);
        
        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        
        // Bind to the address
        let listener = TcpListener::bind(addr).await.map_err(|e| {
            WebServerError::Config(format!("Failed to bind to address: {}", e))
        })?;
        
        // Create a combined future that will resolve when either the server shuts down or we receive a shutdown signal
        tokio::select! {
            result = axum::serve(listener, router) => {
                if let Err(e) = result {
                    error!("Web server error: {}", e);
                    return Err(WebServerError::Server(e.into()));
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Shutting down web server...");
            }
        }
        
        Ok(())
    }
    
    /// Stop the web server
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
            debug!("Shutdown signal sent to web server");
        }
    }
}

/// Serve the index HTML page
async fn serve_index() -> impl IntoResponse {
    match Asset::get("index.html") {
        Some(content) => {
            let html = String::from_utf8_lossy(&content.data).into_owned();
            Html(html).into_response()
        },
        None => {
            tracing::error!("Index HTML not found in embedded assets!");
            (StatusCode::INTERNAL_SERVER_ERROR, "Index not found").into_response()
        }
    }
}

/// Serve embedded assets
async fn serve_embedded_asset(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    tracing::debug!("Requesting asset: {}", path);
    
    // Try to get the asset from the embedded files
    match Asset::get(&path) {
        Some(content) => {
            // Determine content type based on file extension
            let mime_type = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            
            tracing::debug!("Serving asset: {} ({} bytes, type: {})", 
                           path, content.data.len(), mime_type);
            
            AssetResponse::Asset {
                content: content.data.to_vec(),
                content_type: mime_type,
            }
        },
        None => {
            tracing::warn!("Asset not found: {}", path);
            AssetResponse::NotFound
        }
    }
}

/// Get TURN server configuration
async fn get_turn_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(turn_details) = &state.turn_details {
        Json(turn_details.clone()).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}
