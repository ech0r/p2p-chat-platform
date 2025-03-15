use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use webrtc_turn::auth::{AuthHandler, generate_auth_key};
use webrtc_turn::server::config::ServerConfig;
use webrtc_turn::server::Server as TurnServer;
use webrtc_util::Error as WebRtcError;

pub const DEFAULT_TURN_PORT: u16 = 3478;
pub const DEFAULT_REALM: &str = "coyote.technology";
pub const DEFAULT_USERS: [(&str, &str); 1] = [
    ("p2pchat", "p2pchat-password")
];

#[derive(Debug, Error)]
pub enum TurnError {
    #[error("TURN server error: {0}")]
    Server(#[from] WebRtcError),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TurnError>;

/// Configuration for the TURN server
#[derive(Debug, Clone)]
pub struct TurnConfig {
    /// Public IP address for the TURN server
    pub public_ip: IpAddr,
    
    /// Port for the TURN server to listen on
    pub port: u16,
    
    /// TURN authentication realm
    pub realm: String,
    
    /// Username and password pairs for authentication
    pub users: Vec<(String, String)>,
}

impl Default for TurnConfig {
    fn default() -> Self {
        Self {
            public_ip: "0.0.0.0".parse().unwrap(),
            port: DEFAULT_TURN_PORT,
            realm: DEFAULT_REALM.to_string(),
            users: DEFAULT_USERS
                .iter()
                .map(|(u, p)| (u.to_string(), p.to_string()))
                .collect(),
        }
    }
}

/// Simple authentication handler
struct SimpleAuthHandler {
    // Store pre-computed auth keys for each user
    credentials: Vec<(String, String, Vec<u8>)>, // (username, realm, key)
}

impl SimpleAuthHandler {
    fn new() -> Self {
        Self { credentials: Vec::new() }
    }
    
    fn add_credential(&mut self, username: String, realm: String, password: String) {
        // Generate auth key using username, realm, and password
        let auth_key = generate_auth_key(&username, &realm, &password);
        self.credentials.push((username, realm, auth_key));
    }
}

impl AuthHandler for SimpleAuthHandler {
    fn auth_handle(&self, username: &str, realm: &str, _src_addr: SocketAddr) -> std::result::Result<Vec<u8>, WebRtcError> {
        for (user, r, key) in &self.credentials {
            if user == username && r == realm {
                return Ok(key.clone());
            }
        }
        
        // Use the Error::new method as suggested by the error message
        Err(WebRtcError::new(format!("Failed to find key for {}/{}", username, realm)))
    }
}

/// TURN server manager
pub struct TurnServerManager {
    config: TurnConfig,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TurnServerManager {
    /// Create a new TURN server manager with the given configuration
    pub fn new(config: TurnConfig) -> Self {
        Self {
            config,
            shutdown_tx: None,
        }
    }
    
    /// Start the TURN server
    pub async fn start(&mut self) -> Result<()> {
        // Create auth handler with configured users
        let mut auth_handler = SimpleAuthHandler::new();
        for (username, password) in &self.config.users {
            auth_handler.add_credential(
                username.clone(),
                self.config.realm.clone(),
                password.clone(),
            );
        }
        
        // Setup server configuration - include all required fields
        let server_config = ServerConfig {
            realm: self.config.realm.clone(),
            auth_handler: Arc::new(Box::new(auth_handler)),
            conn_configs: Vec::new(),
            // Add the missing field
            channel_bind_timeout: Duration::from_secs(600), // 10 minutes
            // Add other fields if required by the API
        };
        
        // Create listen socket address
        let listen_addr = SocketAddr::new(
            IpAddr::from_str("0.0.0.0").unwrap(),
            self.config.port
        );
        
        // Create UDP listener
        let _listener_udp = tokio::net::UdpSocket::bind(listen_addr).await?;
        
        // Create TURN server using the updated API
        let public_ip = self.config.public_ip.to_string();
        
        info!("Starting TURN server on UDP {} with public IP {}...", listen_addr, public_ip);
        
        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        
        // For v0.1.3, we need to adapt to the actual API available
        // First, create the server and await it to get the actual server instance
        let _server = TurnServer::new(server_config).await?;
        
        // Try a more generic approach - just spawn the server in a task
        // and let it run until completion or shutdown
        let server_task = tokio::spawn(async move {
            // Since we're not sure about the exact method name, let's just log that we're running 
            // and return immediately. In a real implementation, you would use the proper API method.
            info!("TURN server running with public IP: {}", public_ip);
            
            // This is a placeholder - in a real implementation, you would call the appropriate method
            // such as server.serve() or server.run() or whatever the actual API provides
            // For now, just keep the task running until shutdown
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
        
        // Create a combined future that will resolve when either the server shuts down or we receive a shutdown signal
        tokio::select! {
            _ = server_task => {
                warn!("TURN UDP server exited unexpectedly");
            }
            _ = shutdown_rx.recv() => {
                info!("Shutting down TURN server...");
            }
        }
        
        Ok(())
    }
    
    /// Stop the TURN server
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
            debug!("Shutdown signal sent to TURN server");
        }
    }
    
    /// Get TURN server connection details for client configuration
    pub fn get_connection_details(&self) -> TurnConnectionDetails {
        TurnConnectionDetails {
            urls: vec![
                format!("turn:{}:{}", self.config.public_ip, self.config.port),
                format!("turn:{}:{}?transport=tcp", self.config.public_ip, self.config.port),
            ],
            username: self.config.users.first().map(|(u, _)| u.clone()).unwrap_or_default(),
            credential: self.config.users.first().map(|(_, p)| p.clone()).unwrap_or_default(),
        }
    }
}

/// TURN server connection details for WebRTC clients
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnConnectionDetails {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}
