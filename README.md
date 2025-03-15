# P2P Chat Platform

A peer-to-peer encrypted chat platform with an integrated TURN server and web interface.

## Features

- End-to-end encrypted messaging using AES-256-GCM
- Peer-to-peer WebRTC connections for direct communication
- Built-in TURN server for NAT traversal
- Self-contained Rust binary including:
  - TURN server
  - Web server
  - WebAssembly chat application

## Architecture

This project is organized as a Rust workspace with the following components:

- `p2p-chat-wasm` - WebAssembly chat client compiled from Rust
- `turn-server` - TURN server for facilitating WebRTC connections
- `web-server` - Serves the web interface and WASM files
- `p2p-chat-server` - Main binary that integrates all components

## Building

### Requirements

- Rust 1.75 or later
- wasm-pack (for compiling Rust to WebAssembly)
- pkg-config and libssl-dev (for TURN server)

### Build Steps

1. Install wasm-pack:
   ```
   curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
   ```

2. Build the project:
   ```
   cargo build --release
   ```

The build process will:
1. Compile the WASM chat application
2. Bundle it with the web interface
3. Build the TURN server
4. Create a single binary that includes everything

## Running

```
./target/release/p2p-chat-server
```

### Configuration

The server can be configured using command-line arguments or environment variables:

#### TURN Server Configuration
- `--turn-public-ip` or `TURN_PUBLIC_IP` - Public IP address for the TURN server (default: 127.0.0.1)
- `--turn-port` or `TURN_PORT` - Port for the TURN server (default: 3478)
- `--turn-realm` or `TURN_REALM` - Authentication realm (default: coyote.technology)
- `--turn-username` or `TURN_USERNAME` - TURN username (default: p2pchat)
- `--turn-password` or `TURN_PASSWORD` - TURN password (default: p2pchat-password)

#### Web Server Configuration
- `--web-bind-ip` or `WEB_BIND_IP` - IP address to bind the web server to (default: 0.0.0.0)
- `--web-port` or `WEB_PORT` - Port for the web server (default: 8080)
- `--static-dir` or `STATIC_DIR` - Path to static files directory (optional, uses embedded assets by default)

## Docker Deployment

A Dockerfile is provided for easy deployment:

```
docker build -t p2p-chat-platform .
docker run -p 3478:3478/udp -p 3478:3478/tcp -p 8080:8080 -e TURN_PUBLIC_IP=your.public.ip p2p-chat-platform
```

Replace `your.public.ip` with your server's public IP address.

## Deploying to coyote.technology

1. Build the Docker image:
   ```
   docker build -t p2p-chat-platform .
   ```

2. Tag and push to your registry:
   ```
   docker tag p2p-chat-platform registry.coyote.technology/p2p-chat-platform
   docker push registry.coyote.technology/p2p-chat-platform
   ```

3. Deploy using your container orchestration system or directly with:
   ```
   docker run -d --name p2p-chat \
     -p 3478:3478/udp \
     -p 3478:3478/tcp \
     -p 443:8080 \
     -e TURN_PUBLIC_IP=coyote.technology \
     -e TURN_REALM=coyote.technology \
     -e TURN_USERNAME=your_username \
     -e TURN_PASSWORD=your_secure_password \
     registry.coyote.technology/p2p-chat-platform
   ```

## Security Considerations

- Change the default TURN credentials in production
- Consider using HTTPS for the web server
- The encryption keys for chat messages are generated in the browser - consider implementing a more secure key exchange mechanism for sensitive applications

## License

MIT
