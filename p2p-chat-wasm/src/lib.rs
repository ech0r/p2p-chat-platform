use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    console, RtcConfiguration, RtcDataChannelEvent, RtcDataChannelState,
    RtcIceCandidate, RtcIceCandidateInit, RtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcSdpType, RtcSessionDescriptionInit, Request, RequestInit, RequestMode, Response,
};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    
    type Function;
    
    #[wasm_bindgen(method, structural, js_name = call)]
    fn call(this: &Function, thisArg: &JsValue, args: &JsValue) -> JsValue;
    
    #[wasm_bindgen(method, structural, js_name = apply)]
    fn apply(this: &Function, thisArg: &JsValue, args: &JsValue) -> JsValue;
    
    #[wasm_bindgen(method, structural, js_name = call1)]
    fn call1(this: &Function, thisArg: &JsValue, arg1: &JsValue) -> JsValue;
}

#[derive(Serialize, Deserialize)]
struct EncryptedMessage {
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct SessionDescription {
    sdp: String,
    type_: String,
}

#[derive(Serialize, Deserialize)]
struct IceCandidate {
    candidate: String,
    sdp_mid: Option<String>,
    sdp_m_line_index: Option<u16>,
    username_fragment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnConfig {
    urls: Vec<String>,
    username: String,
    credential: String,
}

#[wasm_bindgen]
pub struct P2PChat {
    peer_connection: RtcPeerConnection,
    data_channel: Option<web_sys::RtcDataChannel>,
    encryption_key: [u8; 32],
    on_message_callback: Option<js_sys::Function>,
    on_connection_callback: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl P2PChat {
    #[wasm_bindgen(constructor)]
    pub fn new(turn_config_js: JsValue) -> Result<P2PChat, JsValue> {
        console::log_1(&"Initializing P2P Chat...".into());
        
        // Configure ICE servers (STUN/TURN)
        let rtc_config = RtcConfiguration::new();
        let ice_servers = js_sys::Array::new();
        
        // Add STUN server
        let stun_server = js_sys::Object::new();
        js_sys::Reflect::set(&stun_server, &"urls".into(), &"stun:stun.l.google.com:19302".into())?;
        ice_servers.push(&stun_server);
        
        // Log that we're initializing with a custom TURN config or fallback
        if !turn_config_js.is_null() && !turn_config_js.is_undefined() {
            console::log_1(&"Using provided TURN configuration".into());
        } else {
            console::log_1(&"Using fallback TURN servers".into());
        }
        
        // Add TURN servers if provided in config
        if !turn_config_js.is_null() && !turn_config_js.is_undefined() {
            if let Ok(turn_config) = serde_wasm_bindgen::from_value::<TurnConfig>(turn_config_js) {
                for url in &turn_config.urls {
                    let turn_server = js_sys::Object::new();
                    js_sys::Reflect::set(&turn_server, &"urls".into(), &url.clone().into())?;
                    js_sys::Reflect::set(&turn_server, &"username".into(), &turn_config.username.clone().into())?;
                    js_sys::Reflect::set(&turn_server, &"credential".into(), &turn_config.credential.clone().into())?;
                    ice_servers.push(&turn_server);
                    
                    console::log_1(&format!("Added TURN server: {}", url).into());
                }
            } else {
                console::log_1(&"Invalid TURN configuration format".into());
            }
        } else {
            // Use fallback TURN servers for public deployments
            let turn_server1 = js_sys::Object::new();
            js_sys::Reflect::set(&turn_server1, &"urls".into(), &"turn:relay.metered.ca:80".into())?;
            js_sys::Reflect::set(&turn_server1, &"username".into(), &"openrelayproject".into())?;
            js_sys::Reflect::set(&turn_server1, &"credential".into(), &"openrelayproject".into())?;
            ice_servers.push(&turn_server1);
            
            let turn_server2 = js_sys::Object::new();
            js_sys::Reflect::set(&turn_server2, &"urls".into(), &"turn:relay.metered.ca:443?transport=tcp".into())?;
            js_sys::Reflect::set(&turn_server2, &"username".into(), &"openrelayproject".into())?;
            js_sys::Reflect::set(&turn_server2, &"credential".into(), &"openrelayproject".into())?;
            ice_servers.push(&turn_server2);
        }
        
        rtc_config.set_ice_servers(&ice_servers);
        
        // Create the peer connection
        let peer_connection = RtcPeerConnection::new_with_configuration(&rtc_config)?;
        
        // Generate a random encryption key
        let mut encryption_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut encryption_key);
        
        Ok(P2PChat {
            peer_connection,
            data_channel: None,
            encryption_key,
            on_message_callback: None,
            on_connection_callback: None,
        })
    }
    
    // Set callback for incoming messages
    #[wasm_bindgen]
    pub fn on_message(&mut self, callback: js_sys::Function) {
        self.on_message_callback = Some(callback);
    }
    
    // Set callback for connection status
    #[wasm_bindgen]
    pub fn on_connection(&mut self, callback: js_sys::Function) {
        self.on_connection_callback = Some(callback);
    }
    
    // Create offer as initiator
    #[wasm_bindgen]
    pub async fn create_offer(&mut self) -> Result<JsValue, JsValue> {
        console::log_1(&"Creating offer...".into());
        
        // Configure ICE parameters for better connectivity
        let ice_transport_policy = js_sys::Object::new();
        
        // Set ICE transport policy to "all" to use both relay and direct connections
        js_sys::Reflect::set(&ice_transport_policy, &"iceTransportPolicy".into(), &"all".into())?;
        
        // Create data channel with reliable transport
        let data_channel_init = js_sys::Object::new();
        js_sys::Reflect::set(&data_channel_init, &"ordered".into(), &JsValue::from_bool(true))?;
        
        // Create data channel - using the standard method since the one with dict isn't available
        let data_channel = self.peer_connection.create_data_channel("chat");
        self.setup_data_channel(&data_channel);
        self.data_channel = Some(data_channel);
        
        // Create offer using standard method since the one with options isn't available
        let offer = JsFuture::from(self.peer_connection.create_offer()).await?;
        let offer_sdp = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        let sdp_str = js_sys::Reflect::get(&offer, &"sdp".into())?
            .as_string()
            .unwrap();
        offer_sdp.set_sdp(&sdp_str);
        
        // Set local description
        JsFuture::from(self.peer_connection.set_local_description(&offer_sdp)).await?;
        
        // Setup ICE candidate handling
        self.setup_ice_candidate_handler();
        
        // Convert to a serializable format
        let session_desc = SessionDescription {
            sdp: sdp_str,
            type_: "offer".to_string(),
        };
        
        // Return the offer as a serializable object
        Ok(serde_wasm_bindgen::to_value(&session_desc)?)
    }
    
    // Accept offer as peer
    #[wasm_bindgen]
    pub async fn accept_offer(&mut self, offer: JsValue) -> Result<JsValue, JsValue> {
        console::log_1(&"Accepting offer...".into());
        
        // Create a callback for data channel events
        let encryption_key = self.encryption_key.clone();
        let message_callback = self.on_message_callback.clone();
        let connection_callback = self.on_connection_callback.clone();
        
        // Create a static callback
        let ondatachannel_callback = Closure::wrap(Box::new(move |event: RtcDataChannelEvent| {
            let data_channel = event.channel();
            
            // Message handler (copied from setup_data_channel)
            let msg_callback = message_callback.clone();
            let enc_key = encryption_key.clone();
            let onmessage_cb = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                if let Some(text) = event.data().as_string() {
                    if let Ok(message_obj) = serde_json::from_str::<EncryptedMessage>(&text) {
                        if let (Ok(nonce_bytes), Ok(ciphertext)) = (decode(&message_obj.nonce), decode(&message_obj.ciphertext)) {
                            let cipher = Aes256Gcm::new_from_slice(&enc_key).unwrap();
                            let nonce = Nonce::from_slice(&nonce_bytes);
                            
                            if let Ok(plaintext) = cipher.decrypt(nonce, ciphertext.as_ref()) {
                                if let Ok(decrypted_message) = String::from_utf8(plaintext) {
                                    if let Some(ref callback) = msg_callback {
                                        let this = JsValue::NULL;
                                        let arg = JsValue::from_str(&decrypted_message);
                                        let _ = callback.call1(&this, &arg);
                                    }
                                }
                            }
                        }
                    }
                }
            }) as Box<dyn FnMut(web_sys::MessageEvent)>);
            
            // Open handler
            let conn_cb = connection_callback.clone();
            let onopen_cb = Closure::wrap(Box::new(move |_| {
                if let Some(ref callback) = conn_cb {
                    let this = JsValue::NULL;
                    let arg = JsValue::from_str("open");
                    let _ = callback.call1(&this, &arg);
                }
            }) as Box<dyn FnMut(web_sys::Event)>);
            
            // Close handler
            let conn_cb_close = connection_callback.clone();
            let onclose_cb = Closure::wrap(Box::new(move |_| {
                if let Some(ref callback) = conn_cb_close {
                    let this = JsValue::NULL;
                    let arg = JsValue::from_str("closed");
                    let _ = callback.call1(&this, &arg);
                }
            }) as Box<dyn FnMut(web_sys::Event)>);
            
            data_channel.set_onmessage(Some(onmessage_cb.as_ref().unchecked_ref()));
            data_channel.set_onopen(Some(onopen_cb.as_ref().unchecked_ref()));
            data_channel.set_onclose(Some(onclose_cb.as_ref().unchecked_ref()));
            
            onmessage_cb.forget();
            onopen_cb.forget();
            onclose_cb.forget();
            
            // Store the data channel on the global window object for future use
            let window = web_sys::window().expect("Should have a window");
            js_sys::Reflect::set(
                &window,
                &JsValue::from_str("__wasmDataChannel"),
                &data_channel
            ).expect("Should be able to set global variable");
            
            // Notify connection opened
            if let Some(ref callback) = connection_callback {
                let this = JsValue::NULL;
                let arg = JsValue::from_str("channel_received");
                let _ = callback.call1(&this, &arg);
            }
        }) as Box<dyn FnMut(RtcDataChannelEvent)>);
        
        self.peer_connection.set_ondatachannel(Some(ondatachannel_callback.as_ref().unchecked_ref()));
        ondatachannel_callback.forget();
        
        // Parse the offer and create RtcSessionDescriptionInit
        let offer_data: SessionDescription = serde_wasm_bindgen::from_value(offer)?;
        let offer_sdp = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        offer_sdp.set_sdp(&offer_data.sdp);
        
        // Set remote description
        JsFuture::from(self.peer_connection.set_remote_description(&offer_sdp)).await?;
        
        // Create answer
        let answer = JsFuture::from(self.peer_connection.create_answer()).await?;
        let answer_sdp = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        let sdp_str = js_sys::Reflect::get(&answer, &"sdp".into())?
            .as_string()
            .unwrap();
        answer_sdp.set_sdp(&sdp_str);
        
        // Set local description
        JsFuture::from(self.peer_connection.set_local_description(&answer_sdp)).await?;
        
        // Setup ICE candidate handling
        self.setup_ice_candidate_handler();
        
        // Convert to a serializable format
        let session_desc = SessionDescription {
            sdp: sdp_str,
            type_: "answer".to_string(),
        };
        
        // Return the answer as a serializable object
        Ok(serde_wasm_bindgen::to_value(&session_desc)?)
    }
    
    // Complete connection with answer from peer
    #[wasm_bindgen]
    pub async fn complete_connection(&self, answer: JsValue) -> Result<(), JsValue> {
        console::log_1(&"Completing connection...".into());
        
        // Parse the answer and create RtcSessionDescriptionInit
        let answer_data: SessionDescription = serde_wasm_bindgen::from_value(answer)?;
        let answer_sdp = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        answer_sdp.set_sdp(&answer_data.sdp);
        
        // Set remote description
        JsFuture::from(self.peer_connection.set_remote_description(&answer_sdp)).await?;
        
        Ok(())
    }
    
    // Add ICE candidate received from peer
    #[wasm_bindgen]
    pub async fn add_ice_candidate(&self, candidate: JsValue) -> Result<(), JsValue> {
        let candidate_data: IceCandidate = serde_wasm_bindgen::from_value(candidate)?;
        
        let candidate_init = RtcIceCandidateInit::new(&candidate_data.candidate);
        
        if let Some(sdp_mid) = candidate_data.sdp_mid {
            candidate_init.set_sdp_mid(Some(&sdp_mid));
        }
        
        if let Some(sdp_m_line_index) = candidate_data.sdp_m_line_index {
            candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index));
        }
        
        // Not all browsers support username_fragment
        // So we'll just ignore it if present
        
        let rtc_candidate = RtcIceCandidate::new(&candidate_init)?;
        JsFuture::from(self.peer_connection.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&rtc_candidate))).await?;
        
        Ok(())
    }
    
    // Send encrypted message
    #[wasm_bindgen]
    pub fn send_message(&self, message: String) -> Result<(), JsValue> {
        if let Some(ref channel) = self.data_channel {
            if channel.ready_state() == RtcDataChannelState::Open {
                // Encrypt the message
                let (encrypted_message, nonce) = self.encrypt_message(&message)?;
                
                // Create the message object
                let message_obj = EncryptedMessage {
                    nonce: encode(&nonce),
                    ciphertext: encode(&encrypted_message),
                };
                
                // Serialize to JSON
                let json = serde_json::to_string(&message_obj).map_err(|e| JsValue::from_str(&e.to_string()))?;
                
                // Send through data channel
                channel.send_with_str(&json)?;
                return Ok(());
            }
        }
        
        Err(JsValue::from_str("Data channel not open"))
    }
    
    // Set encryption key from string (base64-encoded)
    #[wasm_bindgen]
    pub fn set_encryption_key(&mut self, key_base64: String) -> Result<(), JsValue> {
        let key_bytes = decode(&key_base64).map_err(|e| JsValue::from_str(&e.to_string()))?;
        
        if key_bytes.len() != 32 {
            return Err(JsValue::from_str("Invalid key length, must be 32 bytes"));
        }
        
        self.encryption_key.copy_from_slice(&key_bytes);
        Ok(())
    }
    
    // Get current encryption key as base64
    #[wasm_bindgen]
    pub fn get_encryption_key(&self) -> String {
        encode(&self.encryption_key)
    }
    
    // Get current connection state
    #[wasm_bindgen]
    pub fn get_connection_state(&self) -> Result<String, JsValue> {
        // Get the signaling state as a string
        let state = match self.peer_connection.signaling_state() {
            web_sys::RtcSignalingState::Stable => "stable",
            web_sys::RtcSignalingState::HaveLocalOffer => "have-local-offer",
            web_sys::RtcSignalingState::HaveRemoteOffer => "have-remote-offer",
            web_sys::RtcSignalingState::HaveLocalPranswer => "have-local-pranswer",
            web_sys::RtcSignalingState::HaveRemotePranswer => "have-remote-pranswer",
            web_sys::RtcSignalingState::Closed => "closed",
            _ => "unknown"
        };
        
        Ok(state.to_string())
    }
    
    // Get WebRTC ICE connection state
    #[wasm_bindgen]
    pub fn get_ice_connection_state(&self) -> String {
        // Convert RtcIceConnectionState to a string manually
        match self.peer_connection.ice_connection_state() {
            web_sys::RtcIceConnectionState::New => "new".to_string(),
            web_sys::RtcIceConnectionState::Checking => "checking".to_string(),
            web_sys::RtcIceConnectionState::Connected => "connected".to_string(),
            web_sys::RtcIceConnectionState::Completed => "completed".to_string(),
            web_sys::RtcIceConnectionState::Failed => "failed".to_string(),
            web_sys::RtcIceConnectionState::Disconnected => "disconnected".to_string(),
            web_sys::RtcIceConnectionState::Closed => "closed".to_string(),
            _ => "unknown".to_string()
        }
    }
    
    // Get WebRTC ICE gathering state
    #[wasm_bindgen]
    pub fn get_ice_gathering_state(&self) -> String {
        // For ice_gathering_state, we'll use a fallback approach
        // since the method isn't directly available in the current web_sys version
        "unknown".to_string()
    }
    
    // Setup data channel handlers
    fn setup_data_channel(&self, channel: &web_sys::RtcDataChannel) {
        let callback_clone = self.on_message_callback.clone();
        let encryption_key = self.encryption_key.clone();
        
        // Message handler
        let onmessage_callback = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            if let Some(text) = event.data().as_string() {
                // Parse the message
                if let Ok(message_obj) = serde_json::from_str::<EncryptedMessage>(&text) {
                    // Decode base64
                    if let (Ok(nonce_bytes), Ok(ciphertext)) = (decode(&message_obj.nonce), decode(&message_obj.ciphertext)) {
                        // Decrypt the message
                        let cipher = Aes256Gcm::new_from_slice(&encryption_key).unwrap();
                        let nonce = Nonce::from_slice(&nonce_bytes);
                        
                        if let Ok(plaintext) = cipher.decrypt(nonce, ciphertext.as_ref()) {
                            // Convert bytes to string
                            if let Ok(decrypted_message) = String::from_utf8(plaintext) {
                                // Call the JavaScript callback
                                if let Some(ref callback) = callback_clone {
                                    let this = JsValue::NULL;
                                    let arg = JsValue::from_str(&decrypted_message);
                                    let _ = callback.call1(&this, &arg);
                                }
                            }
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        
        // Connection open handler
        let conn_callback = self.on_connection_callback.clone();
        let onopen_callback = Closure::wrap(Box::new(move |_| {
            if let Some(ref callback) = conn_callback {
                let this = JsValue::NULL;
                let arg = JsValue::from_str("open");
                let _ = callback.call1(&this, &arg);
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        
        // Connection close handler
        let conn_callback_close = self.on_connection_callback.clone();
        let onclose_callback = Closure::wrap(Box::new(move |_| {
            if let Some(ref callback) = conn_callback_close {
                let this = JsValue::NULL;
                let arg = JsValue::from_str("closed");
                let _ = callback.call1(&this, &arg);
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        
        channel.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        channel.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
        channel.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
        
        onmessage_callback.forget();
        onopen_callback.forget();
        onclose_callback.forget();
    }
    
    // Setup ICE candidate handling
    fn setup_ice_candidate_handler(&self) {
        let connection_clone = self.on_connection_callback.clone();
        
        let onicecandidate_callback = Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
            if let Some(candidate) = event.candidate() {
                // Extract necessary properties from the candidate
                if let (Ok(candidate_str), Ok(sdp_mid), Ok(line_index)) = (
                    js_sys::Reflect::get(&candidate, &"candidate".into()),
                    js_sys::Reflect::get(&candidate, &"sdpMid".into()),
                    js_sys::Reflect::get(&candidate, &"sdpMLineIndex".into()),
                ) {
                    let candidate_data = IceCandidate {
                        candidate: candidate_str.as_string().unwrap_or_default(),
                        sdp_mid: sdp_mid.as_string(),
                        sdp_m_line_index: line_index.as_f64().map(|n| n as u16),
                        username_fragment: None,
                    };
                    
                    // Notify about the new ICE candidate
                    if let Some(ref callback) = connection_clone {
                        if let Ok(json) = serde_json::to_string(&candidate_data) {
                            let this = JsValue::NULL;
                            let args = js_sys::Array::new();
                            args.push(&JsValue::from_str("ice_candidate"));
                            args.push(&JsValue::from_str(&json));
                            let _ = callback.apply(&this, &args);
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);
        
        self.peer_connection.set_onicecandidate(Some(onicecandidate_callback.as_ref().unchecked_ref()));
        onicecandidate_callback.forget();
    }
    
    // Encrypt message
    fn encrypt_message(&self, message: &str) -> Result<(Vec<u8>, Vec<u8>), JsValue> {
        // Create cipher
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        
        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Encrypt
        let ciphertext = cipher.encrypt(nonce, message.as_bytes())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        
        Ok((ciphertext, nonce_bytes.to_vec()))
    }
}

// Fetch TURN configuration from the server
#[wasm_bindgen]
pub async fn fetch_turn_config() -> Result<JsValue, JsValue> {
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    
    let request = Request::new_with_str_and_init("/api/turn-config", &opts)?;
    
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No window found"))?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;
    
    if !resp.ok() {
        return Err(JsValue::from_str("Failed to fetch TURN configuration"));
    }
    
    let json = JsFuture::from(resp.json()?).await?;
    Ok(json)
}

// Initialize the WASM module
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console::log_1(&"P2P Encrypted Chat WASM module initialized".into());
    Ok(())
}
