# veilnet

Networking abstractions built on Veilid API primitives.

veilnet provides high-level networking capabilities using the Veilid distributed hash table and private routing system. It enables applications to communicate over the Veilid network without dealing directly with low-level Veilid APIs.

## Features

- **Anonymous communication** - Messages are routed through Veilid's private route system
- **DHT-based addressing** - Services are addressed using distributed hash table records
- **Datagram-style messaging** - UDP-like message passing with automatic route management
- **Connection recovery** - Built-in handling of network failures and route changes

## Quick Start

Add veilnet to your `Cargo.toml`:

```toml
[dependencies]
veilnet = "0.1.0"
```

### Setting up a listener

```rust
use veilnet::{Connection, datagram::listener::Listener};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::new().await?;
    let mut listener = Listener::bind(conn, None, 8080).await?;

    println!("Listening at: {}", listener.addr());

    // Receive messages
    loop {
        let (sender_route, message) = listener.recv_from().await?;
        println!("Received: {:?}", String::from_utf8_lossy(&message));
    }
}
```

### Sending messages

```rust
use veilnet::{Connection, DHTAddr, datagram::dialer::Dialer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::new().await?;
    let mut dialer = Dialer::new(conn).await?;

    // Parse the listener's address
    let addr: DHTAddr = "VLD0:abcd1234...efgh5678:8080".parse()?;

    // Send a message
    let route_id = dialer.resolve(addr).await?;
    dialer.send_to(route_id, b"Hello, world!".to_vec()).await?;

    Ok(())
}
```

## Example: vcat

The `vcat` example demonstrates a simple netcat-like tool for Veilid networks:

```bash
# Terminal 1 - Start a listener
cargo run --example vcat

# Terminal 2 - Connect and send messages
cargo run --example vcat VLD0:abcd...efgh:0
```

## Architecture

veilnet is built around three core concepts:

- **Connection** - Manages the Veilid network connection and attachment state
- **DHTAddr** - Addresses combining a DHT record key with a subkey (port)
- **Datagram** - UDP-like datagram abstraction with listeners and dialers

The library handles the complexity of Veilid's routing system while providing familiar networking patterns. Messages are automatically encrypted and routed through the Veilid network's onion routing system.

## Error Handling

veilnet provides comprehensive error handling primitives supporting automatic recovery for common network issues:

- Route failures trigger automatic re-resolution
- Connection drops initiate reconnection attempts
- Attachment state changes are monitored continuously

See the `vcat` example for examples of handling these errors in practice.

## License

MPL-2.0
