use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:warning=Using minimal build script (skips WASM compilation)");
    
    // Get the output directory for static files
    let out_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let static_dir = out_dir.join("static");
    let assets_dir = static_dir.join("assets");
    
    // Create directories if they don't exist
    ensure_dir(&static_dir);
    ensure_dir(&assets_dir);
    
    // Create placeholder files to ensure embedding works
    create_placeholder_files(&static_dir, &assets_dir);
    
    println!("cargo:warning=Static directory structure created");
    println!("cargo:warning=Use 'make build-wasm' to compile the WebAssembly module");
}

fn ensure_dir(dir: &Path) {
    if !dir.exists() {
        println!("cargo:warning=Creating directory: {}", dir.display());
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to create directory {}: {}", dir.display(), e);
        });
    }
}

fn create_placeholder_files(static_dir: &Path, assets_dir: &Path) {
    // Create minimal index.html if it doesn't exist
    let index_path = static_dir.join("index.html");
    if !index_path.exists() {
        let index_content = r#"<!DOCTYPE html>
<html>
c<head>
    <title>P2P Chat Platform</title>
    <meta charset="UTF-8">
</head>
<body>
    <h1>P2P Chat Platform</h1>
    <p>WASM module not compiled. Run 'make build-wasm' first.</p>
</body>
</html>"#;
        fs::write(&index_path, index_content).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to write index.html: {}", e);
        });
    }
    
    // Create placeholder JS
    let js_path = assets_dir.join("p2p_chat_wasm.js");
    if !js_path.exists() {
        let js_content = "// Placeholder for WASM module\nconsole.error('WASM module not compiled. Run make build-wasm first.');\n";
        fs::write(&js_path, js_content).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to write placeholder JS: {}", e);
        });
    }
    
    // Create empty WASM file
    let wasm_path = assets_dir.join("p2p_chat_wasm_bg.wasm");
    if !wasm_path.exists() {
        fs::write(&wasm_path, &[0u8; 8]).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to write placeholder WASM: {}", e);
        });
    }
}
