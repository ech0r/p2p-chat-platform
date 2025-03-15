.PHONY: all build clean build-wasm build-server run test

# Default target
all: build

# Ensure wasm-pack is installed
check-wasm-pack:
	@which wasm-pack > /dev/null || (echo "wasm-pack not found. Install with: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh" && exit 1)

# Build WASM module separately
build-wasm: check-wasm-pack
	@echo "Building WASM module..."
	cd p2p-chat-wasm && wasm-pack build --target web --out-dir ../p2p-chat-server/static/assets

# Build the server (which will also build WASM via build.rs)
build-server:
	@echo "Building server..."
	cargo build --package p2p-chat-server

# Build everything
build: build-server

# Build release version
release:
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
	RUST_LOG=debug cargo run --package p2p-chat-server

# Run tests
test:
	@echo "Running tests..."
	cargo test --workspace
