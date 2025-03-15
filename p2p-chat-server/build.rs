use std::process::Command;
use std::env;
use std::path::Path;
use std::fs;

fn main() {
    println!("cargo:warning=Building static files for embedding...");
    
    // Always rebuild if the WASM source or Cargo.toml changes
    println!("cargo:rerun-if-changed=../p2p-chat-wasm/src/");
    println!("cargo:rerun-if-changed=../p2p-chat-wasm/Cargo.toml");
    
    // Also rebuild if our static files change
    println!("cargo:rerun-if-changed=../static/");
    
    // Get the current profile (debug or release)
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    println!("cargo:warning=Building with profile: {}", profile);
    
    // Ensure static assets directory exists
    let static_dir = "../static";
    let assets_dir = "../static/assets";
    
    // Create directories if they don't exist
    ensure_dir(static_dir);
    ensure_dir(assets_dir);
    
    // Build the WASM component
    println!("cargo:warning=Building WASM module...");
    
    // Set build mode based on profile
    let build_mode = if profile == "release" { "--release" } else { "--dev" };
    let status = Command::new("wasm-pack")
        .current_dir("../p2p-chat-wasm")
        .arg("build")
        .arg("--target")
        .arg("web")
        .arg("--out-dir")
        .arg("../static/assets")
        .arg(build_mode)
        .arg("--no-typescript")
        .status();
    
    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!("cargo:warning=Successfully built WASM module");
            } else {
                let code = exit_status.code().unwrap_or(-1);
                println!("cargo:warning=wasm-pack failed with exit code: {}", code);
                panic!("wasm-pack build failed with exit code: {}", code);
            }
        },
        Err(e) => {
            println!("cargo:warning=Failed to execute wasm-pack: {}", e);
            panic!("Failed to execute wasm-pack: {}", e);
        }
    }
    
    // Check that the expected files exist
    let js_file = Path::new(assets_dir).join("p2p_chat_wasm.js");
    let wasm_file = Path::new(assets_dir).join("p2p_chat_wasm_bg.wasm");
    
    if !js_file.exists() {
        println!("cargo:warning=JS file not found: {}", js_file.display());
        panic!("JS file not found: {}", js_file.display());
    }
    
    if !wasm_file.exists() {
        println!("cargo:warning=WASM file not found: {}", wasm_file.display());
        panic!("WASM file not found: {}", wasm_file.display());
    }
    
    println!("cargo:warning=Static files ready for embedding!");
    
    // List generated files for debugging
    if let Ok(entries) = fs::read_dir(assets_dir) {
        println!("cargo:warning=Generated static files:");
        for entry in entries {
            if let Ok(entry) = entry {
                println!("cargo:warning=  - {}", entry.path().display());
            }
        }
    }
}

fn ensure_dir(dir: &str) {
    let path = Path::new(dir);
    if !path.exists() {
        println!("cargo:warning=Creating directory: {}", dir);
        fs::create_dir_all(path).unwrap_or_else(|e| {
            panic!("Failed to create directory {}: {}", dir, e);
        });
    }
}
