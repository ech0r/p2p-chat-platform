use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use axum::{
    extract::{Path as AxumPath, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, get_service},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use http::header;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, broadcast};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{debug, error, info};
use turn_server::TurnConnectionDetails;
use rust_embed::RustEmbed;
use uuid::Uuid;
use mime_guess::mime;

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

// Define message types for signaling
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SignalMessage {
    #[serde(rename = "register")]
    Register {
        display_name: String,
    },
    #[serde(rename = "discover")]
    Discover,
    #[serde(rename = "offer")]
    Offer {
        target_user_id: String,
        offer: serde_json::Value,
    },
    #[serde(rename = "answer")]
    Answer {
        target_user_id: String,
        answer: serde_json::Value,
    },
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        target_user_id: String,
        candidate: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "registered")]
    Registered {
        user_id: String,
    },
    #[serde(rename = "user_list")]
    UserList {
        users: Vec<UserInfo>,
    },
    #[serde(rename = "user_joined")]
    UserJoined {
        user_id: String,
        display_name: String,
    },
    #[serde(rename = "user_left")]
    UserLeft {
        user_id: String,
    },
    #[serde(rename = "offer")]
    Offer {
        from_user_id: String,
        offer: serde_json::Value,
    },
    #[serde(rename = "answer")]
    Answer {
        from_user_id: String,
        answer: serde_json::Value,
    },
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        from_user_id: String,
        candidate: serde_json::Value,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserInfo {
    user_id: String,
    display_name: String,
}

// Signaling server state
struct SignalingState {
    // Map of user_id to (display_name, sender)
    users: HashMap<String, (String, broadcast::Sender<String>)>,
}

impl SignalingState {
    fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }
}

/// Web server state
#[derive(Clone)]
struct AppState {
    turn_details: Option<TurnConnectionDetails>,
    signaling: Arc<Mutex<SignalingState>>,
}

impl AppState {
    fn new(turn_details: Option<TurnConnectionDetails>) -> Self {
        Self {
            turn_details,
            signaling: Arc::new(Mutex::new(SignalingState::new())),
        }
    }
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
        let state = Arc::new(AppState::new(self.config.turn_details.clone()));
        
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
                .route("/ws", get(handle_ws_connection))
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
                .route("/ws", get(handle_ws_connection))
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

/// Serve embedded assets with proper MIME types
async fn serve_embedded_asset(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    info!("Requesting asset: {}", path);
    let path_actual = format!("assets/{}", path);
    info!("path actual: {}", path_actual);
    // Try to get the asset from the embedded files
    match Asset::get(&path_actual) {
        Some(content) => {
            // Determine content type based on file extension
            let mime_type = match path.rsplit('.').next() {
                Some("js") => mime::APPLICATION_JAVASCRIPT.to_string(),
                Some("wasm") => "application/wasm".to_string(),
                Some("html") => mime::TEXT_HTML.to_string(),
                Some("css") => mime::TEXT_CSS.to_string(),
                Some("png") => mime::IMAGE_PNG.to_string(),
                Some("jpg") | Some("jpeg") => mime::IMAGE_JPEG.to_string(),
                Some("svg") => mime::IMAGE_SVG.to_string(),
                Some("json") => mime::APPLICATION_JSON.to_string(),
                _ => mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string(),
            };
            
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

/// Handle WebSocket connection for signaling
async fn handle_ws_connection(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Accept the WebSocket connection
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle WebSocket after upgrade
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    // Split the socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();
    
    let user_id = Uuid::new_v4().to_string();
    let (tx, mut rx) = broadcast::channel(100);
    
    // Task to receive messages from other users and forward them to this WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });
    
    // Task to receive messages from this WebSocket and process them
    let mut recv_task = tokio::spawn(async move {
        let mut user_registered = false;
        
        while let Some(Ok(message)) = receiver.next().await {
            if let Message::Text(text) = message {
                match serde_json::from_str::<SignalMessage>(&text) {
                    Ok(message) => {
                        match message {
                            SignalMessage::Register { display_name } => {
                                let mut signaling = state.signaling.lock().unwrap();
                                
                                // Register the user
                                signaling.users.insert(user_id.clone(), (display_name.clone(), tx.clone()));
                                user_registered = true;
                                
                                // Send confirmation to the user
                                let registered_msg = serde_json::to_string(&ServerMessage::Registered {
                                    user_id: user_id.clone(),
                                }).unwrap();
                                let _ = tx.send(registered_msg);
                                
                                // Send user list to the new user
                                let users: Vec<UserInfo> = signaling.users.iter()
                                    .map(|(id, (name, _))| UserInfo {
                                        user_id: id.clone(),
                                        display_name: name.clone(),
                                    })
                                    .collect();
                                
                                let user_list_msg = serde_json::to_string(&ServerMessage::UserList {
                                    users,
                                }).unwrap();
                                let _ = tx.send(user_list_msg);
                                
                                // Notify other users about the new user
                                let user_joined_msg = serde_json::to_string(&ServerMessage::UserJoined {
                                    user_id: user_id.clone(),
                                    display_name,
                                }).unwrap();
                                
                                for (id, (_, other_tx)) in signaling.users.iter() {
                                    if id != &user_id {
                                        let _ = other_tx.send(user_joined_msg.clone());
                                    }
                                }
                            },
                            SignalMessage::Discover => {
                                if !user_registered {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Not registered".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                    continue;
                                }
                                
                                let signaling = state.signaling.lock().unwrap();
                                
                                // Send user list
                                let users: Vec<UserInfo> = signaling.users.iter()
                                    .map(|(id, (name, _))| UserInfo {
                                        user_id: id.clone(),
                                        display_name: name.clone(),
                                    })
                                    .collect();
                                
                                let user_list_msg = serde_json::to_string(&ServerMessage::UserList {
                                    users,
                                }).unwrap();
                                let _ = tx.send(user_list_msg);
                            },
                            SignalMessage::Offer { target_user_id, offer } => {
                                if !user_registered {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Not registered".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                    continue;
                                }
                                
                                let signaling = state.signaling.lock().unwrap();
                                
                                // Forward offer to target user
                                if let Some((_, target_tx)) = signaling.users.get(&target_user_id) {
                                    let offer_msg = serde_json::to_string(&ServerMessage::Offer {
                                        from_user_id: user_id.clone(),
                                        offer,
                                    }).unwrap();
                                    let _ = target_tx.send(offer_msg);
                                } else {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Target user not found".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                }
                            },
                            SignalMessage::Answer { target_user_id, answer } => {
                                if !user_registered {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Not registered".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                    continue;
                                }
                                
                                let signaling = state.signaling.lock().unwrap();
                                
                                // Forward answer to target user
                                if let Some((_, target_tx)) = signaling.users.get(&target_user_id) {
                                    let answer_msg = serde_json::to_string(&ServerMessage::Answer {
                                        from_user_id: user_id.clone(),
                                        answer,
                                    }).unwrap();
                                    let _ = target_tx.send(answer_msg);
                                } else {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Target user not found".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                }
                            },
                            SignalMessage::IceCandidate { target_user_id, candidate } => {
                                if !user_registered {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Not registered".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                    continue;
                                }
                                
                                let signaling = state.signaling.lock().unwrap();
                                
                                // Forward ICE candidate to target user
                                if let Some((_, target_tx)) = signaling.users.get(&target_user_id) {
                                    let candidate_msg = serde_json::to_string(&ServerMessage::IceCandidate {
                                        from_user_id: user_id.clone(),
                                        candidate,
                                    }).unwrap();
                                    let _ = target_tx.send(candidate_msg);
                                } else {
                                    let error_msg = serde_json::to_string(&ServerMessage::Error {
                                        message: "Target user not found".into(),
                                    }).unwrap();
                                    let _ = tx.send(error_msg);
                                }
                            },
                        }
                    },
                    Err(e) => {
                        let error_msg = serde_json::to_string(&ServerMessage::Error {
                            message: format!("Invalid message format: {}", e),
                        }).unwrap();
                        let _ = tx.send(error_msg);
                    }
                }
            }
        }
        
        // User disconnected, remove from users map and notify others
        {
            let mut signaling = state.signaling.lock().unwrap();
            signaling.users.remove(&user_id);
            
            // Notify other users
            let user_left_msg = serde_json::to_string(&ServerMessage::UserLeft {
                user_id: user_id.clone(),
            }).unwrap();
            
            for (id, (_, other_tx)) in signaling.users.iter() {
                if id != &user_id {
                    let _ = other_tx.send(user_left_msg.clone());
                }
            }
        }
        
        // The user disconnected
        tracing::debug!("User {} disconnected", user_id);
    });
    
    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }
}
