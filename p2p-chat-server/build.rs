use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../p2p-chat-wasm/src");
    println!("cargo:rerun-if-changed=../p2p-chat-wasm/Cargo.toml");
    println!("cargo:rerun-if-changed=static/index.html");
    
    // Get the output directory for static files
    let out_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let static_dir = out_dir.join("static");
    let assets_dir = static_dir.join("assets");
    
    // Create directories if they don't exist
    ensure_dir(&static_dir);
    ensure_dir(&assets_dir);
    
    // Check if we should compile WASM
    // Allow skipping WASM compilation with an environment variable
    let skip_wasm = env::var("SKIP_WASM_COMPILATION").unwrap_or_default() == "1";
    
    if skip_wasm {
        println!("cargo:warning=Skipping WASM compilation (SKIP_WASM_COMPILATION=1)");
        create_placeholder_files(&static_dir, &assets_dir);
    } else {
        // Try to compile WASM module
        println!("cargo:warning=Compiling WebAssembly module...");
        
        // First, ensure wasm-pack is installed
        let wasm_pack_check = Command::new("wasm-pack")
            .args(&["--version"])
            .output();
            
        match wasm_pack_check {
            Ok(output) => {
                if output.status.success() {
                    println!("cargo:warning=Found wasm-pack: {}", String::from_utf8_lossy(&output.stdout).trim());
                } else {
                    println!("cargo:warning=wasm-pack check failed: {}", String::from_utf8_lossy(&output.stderr));
                    install_wasm_pack();
                }
            },
            Err(e) => {
                println!("cargo:warning=Failed to check wasm-pack: {}", e);
                install_wasm_pack();
            }
        }
        
        // Build the WebAssembly module
        let wasm_dir = PathBuf::from("../p2p-chat-wasm");
        let output_dir = PathBuf::from("../p2p-chat-server/static/assets");
        
        // Make sure output directory exists and is empty
        ensure_dir(&assets_dir);
        
        println!("cargo:warning=Building WebAssembly module from {} to {}", 
            wasm_dir.display(), output_dir.display());
        
        let status = Command::new("wasm-pack")
            .current_dir(&wasm_dir)
            .args(&["build", "--target", "web", "--out-dir", output_dir.to_str().unwrap()])
            .status();
        
        match status {
            Ok(exit_status) if exit_status.success() => {
                println!("cargo:warning=WebAssembly module compiled successfully");
                // Fix the module name in the generated files
                fix_wasm_module_names(&assets_dir);
            },
            Ok(exit_status) => {
                println!("cargo:warning=Failed to compile WebAssembly module: {:?}", exit_status);
                create_placeholder_files(&static_dir, &assets_dir);
            },
            Err(e) => {
                println!("cargo:warning=Failed to run wasm-pack: {}", e);
                println!("cargo:warning=Make sure wasm-pack is installed: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh");
                create_placeholder_files(&static_dir, &assets_dir);
            }
        }
    }
}

fn ensure_dir(dir: &Path) {
    if !dir.exists() {
        println!("cargo:warning=Creating directory: {}", dir.display());
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to create directory {}: {}", dir.display(), e);
        });
    }
}

fn install_wasm_pack() {
    println!("cargo:warning=Attempting to install wasm-pack...");
    
    let status = Command::new("curl")
        .args(&[
            "https://rustwasm.github.io/wasm-pack/installer/init.sh",
            "-sSf",
            "|",
            "sh"
        ])
        .status();
        
    match status {
        Ok(exit_status) if exit_status.success() => {
            println!("cargo:warning=Successfully installed wasm-pack");
        },
        Ok(exit_status) => {
            println!("cargo:warning=Failed to install wasm-pack: {:?}", exit_status);
        },
        Err(e) => {
            println!("cargo:warning=Failed to run installer: {}", e);
        }
    }
}

fn fix_wasm_module_names(assets_dir: &Path) {
    // wasm-pack generates files with the package name as a prefix
    // we want to rename them to have predictable names
    let package_name = "p2p_chat_wasm";
    
    let js_file = assets_dir.join(format!("{}.js", package_name));
    let wasm_file = assets_dir.join(format!("{}_bg.wasm", package_name));
    
    if js_file.exists() && wasm_file.exists() {
        println!("cargo:warning=Found WebAssembly files with expected names");
        // No need to rename
    } else {
        println!("cargo:warning=Looking for WebAssembly files with different names...");
        
        // Find and rename files
        if let Ok(entries) = fs::read_dir(assets_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                
                if file_name.ends_with(".js") && !file_name.ends_with(".d.js") && file_name != format!("{}.js", package_name) {
                    let new_name = format!("{}.js", package_name);
                    println!("cargo:warning=Renaming {} to {}", file_name, new_name);
                    let _ = fs::rename(&path, assets_dir.join(new_name));
                } else if file_name.ends_with("_bg.wasm") && file_name != format!("{}_bg.wasm", package_name) {
                    let new_name = format!("{}_bg.wasm", package_name);
                    println!("cargo:warning=Renaming {} to {}", file_name, new_name);
                    let _ = fs::rename(&path, assets_dir.join(new_name));
                }
            }
        }
    }
}

fn create_placeholder_files(static_dir: &Path, assets_dir: &Path) {
    // Only create placeholders if files don't exist
    let index_path = static_dir.join("index.html");
    let js_path = assets_dir.join("p2p_chat_wasm.js");
    let wasm_path = assets_dir.join("p2p_chat_wasm_bg.wasm");
    
    // Check if any of the files are missing
    let files_missing = !index_path.exists() || !js_path.exists() || !wasm_path.exists();
    
    if files_missing {
        println!("cargo:warning=Creating placeholder files for WASM module");
        
        // Create minimal index.html if it doesn't exist
        if !index_path.exists() {
            let index_content = r#"<!DOCTYPE html>
<html>
<head>
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
        if !js_path.exists() {
            let js_content = r#"// Placeholder for WASM module
console.error('WASM module not compiled. Run make build-wasm first.');

// Export placeholder objects
export function init() {
  return Promise.resolve();
}

export class P2PChat {
  constructor() {
    console.error('WASM module not compiled. Run make build-wasm first.');
  }
  
  on_message() {}
  on_connection() {}
  create_offer() { return Promise.resolve({}); }
  accept_offer() { return Promise.resolve({}); }
  complete_connection() { return Promise.resolve(); }
  add_ice_candidate() { return Promise.resolve(); }
  send_message() { return false; }
  get_encryption_key() { return ''; }
  get_connection_state() { return 'error'; }
  get_ice_connection_state() { return 'error'; }
}

export function fetch_turn_config() {
  return Promise.resolve(null);
}

export default {
  init,
  P2PChat,
  fetch_turn_config
};
"#;
            fs::write(&js_path, js_content).unwrap_or_else(|e| {
                println!("cargo:warning=Failed to write placeholder JS: {}", e);
            });
        }
        
        // Create empty WASM file
        if !wasm_path.exists() {
            fs::write(&wasm_path, &[0u8; 8]).unwrap_or_else(|e| {
                println!("cargo:warning=Failed to write placeholder WASM: {}", e);
            });
        }
    } else {
        println!("cargo:warning=Using existing placeholder files");
    }
}
