.PHONY: all build clean build-wasm build-server run test dev release

# Default target
all: build

# Ensure wasm-pack is installed
check-wasm-pack:
	@which wasm-pack > /dev/null || (echo "wasm-pack not found. Installing with: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh" && curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh)

# Build WASM module separately
build-wasm: check-wasm-pack
	@echo "Building WASM module..."
	cd p2p-chat-wasm && wasm-pack build --target web --out-dir ../p2p-chat-server/static/assets
	@echo "Checking WASM output files..."
	@ls -la p2p-chat-server/static/assets/
	@echo "WASM module built successfully."

# Build just the server (without WASM compilation)
build-server-only:
	@echo "Building server only (no WASM)..."
	SKIP_WASM_COMPILATION=1 cargo build --package p2p-chat-server

# Build the server with WASM
build-server: build-wasm
	@echo "Building server with WASM included..."
	cargo build --package p2p-chat-server

# Build everything
build: build-wasm build-server

# Build release version
release: build-wasm
	@echo "Building release version..."
	cargo build --release --package p2p-chat-server

# Run the server
run: build
	@echo "Running server..."
	cargo run --package p2p-chat-server

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf p2p-chat-server/static/assets/*

# Build and run with debug logging
dev: build
	@echo "Running in development mode..."
	RUST_LOG=debug cargo run --package p2p-chat-server -- --debug

# Run tests
test:
	@echo "Running tests..."
	cargo test --workspace
