# SMS Server

Self-hosted SMS gateway for messaging, GNSS data, and modem control (signal strength, URCs, etc.).
Offers database persistence, full OpenAPI support, delivery tracking, and real-time events—no SIM polling needed.
Secure, flexible, and dependable.

### **Multiple Integration Options**
- **[Rust Client Library](https://github.com/morgverd/sms-client)** (**[crates.io](https://crates.io/crates/sms-client)**) for easily using the HTTP & WebSocket interfaces
- **[HTTP OpenAPI](https://morgverd.github.io/sms-server)** for sending and reading SMS messages, modem requests and device info with OpenAPI support
- **[HTTP Webhooks](docs/events.md)** to receive events with a HTTP server, sending POST requests to provided URLs
- **[WebSocket](docs/websocket.md)** for receiving events live, with optional event filtering

### Built-in Security
- Encryption by default for all message storage within database
- Optional Authorization token for HTTP API and WebSocket connections
- HTTPS/WSS support for servers, custom TLS certificate support for webhooks

### Advanced SMS Features
- Automatic handling of multipart SMS messages
- SMS delivery report tracking with status updates
- International phone number format handling

### Location Services
- Built-in GNSS/GPS location tracking (configurable)
- Real-time position reporting via events
- Location data integration with SMS workflows

## Getting Started

1. **Hardware Setup**: Connect your GSM modem to your device
2. **Configuration**: Create a `config.toml` file (see [Configuration Guide](docs/configuration.md))
3. **Launch**: Start the gateway and begin sending/receiving SMS messages

## Documentation

| Document                                                        | Description                                        |
|-----------------------------------------------------------------|----------------------------------------------------|
| [OpenAPI Documentation](https://morgverd.github.io/sms-server/) | HTTP OpenAPI UI from latest build                  |
| [Configuration Guide](docs/configuration.md)                    | Complete configuration reference with examples     |
| [Event Types](docs/events.md)                                   | Available events received via WebSocket or Webhook |
| [HTTP API Reference](docs/http.md)                              | REST API endpoints for SMS operations              |
| [WebSocket Guide](docs/websocket.md)                            | Real-time event streaming setup                    |

## Features

The feature code is used in the build version metadata suffix. Eg: `1.0.0#ghtr` (GPIO, http-server, tls-rustls).

| Name          | Code | Default | Description                                                                            |
|---------------|------|---------|----------------------------------------------------------------------------------------|
| `gpio`        | `g`  | ✔️      | GPIO power pin support for automatic HAT power management                              |
| `http-server` | `h`  | ✔️      | HTTP server to control the modem and access database                                   |
| `db-sqlite`   |      | ✔️      | SQLite database connection driver (currently only database supported)                  |
| `tls-rustls`  | `tr` | ✔️      | Uses rustls and aws-lc-rs for TLS all connections                                      | 
| `tls-native`  | `tn` |         | Uses openssl for http-server (if enabled) and native-tls for all other TLS connections |
| `openapi`     | `o`  |         | Adds utoipa OpenAPI spec generation to HTTP routes, includes redoc-ui at /docs!        |
| `sentry`      | `s`  |         | Adds Sentry error reporting / logging integration                                      |

## Examples

### [💬 ChatGPT SMS Bot](./examples/chatgpt-sms)

An intelligent SMS responder that integrates with OpenAI's ChatGPT API. Receives incoming messages via webhooks, generates contextual replies using conversation history, and responds automatically. Features conversation memory and customizable response templates.

> [!NOTE]
> Possibly the first ChatGPT SMS implementation running directly through cellular modem hardware!

### [🗺️ Real-time GNSS Viewer](./examples/gnss-viewer)

A web-based GPS tracking dashboard that connects via WebSocket to display live position updates. Monitor location accuracy, track movement patterns, and analyze GPS performance in real-time. Accessible from any networked device with a modern web browser.

### [📟 SMS Terminal](https://github.com/morgverd/sms-terminal) ([crates.io](https://crates.io/crates/sms-terminal))

A Rust TUI that makes it easy to view a phonebook of recent contacts, compose SMS messages, view messages (live updating) and see device info. Uses the [sms-client](https://github.com/morgverd/sms-client) ([crates.io](https://crates.io/crates/sms-client)) library to interface with this project!

## Installation

```shell
git clone https://github.com/morgverd/sms-server

# Build, with all default.
cargo build -r

# Build with Sentry error forwarding.
cargo build -r --features sentry

# Build without HTTP server, and with GPIO, SQLite and Rust TLS.
cargo build -r --no-default-features -F gpio,db-sqlite,tls-rustls

# Build with native SSL and default features.
cargo build -r --no-default-features -F gpio,http-server,db-sqlite,tls-native 
```
```shell
# Show command line help.
./sms-server -h

# Start the SMS server with a config path, can be relative or absolute.
./sms-server -c config.toml

# Start the SMS server with debug logging.
RUST_LOG=debug ./sms-server -c config.toml
```
## Hardware Requirements

You'll need some form of GSM modem that allows for serial connection.
I use (and this project has only been tested with) a [Waveshare GSM Pi Hat](https://www.waveshare.com/gsm-gprs-gnss-hat.htm) on a Raspberry Pi.

> [!TIP]
> Many SIM cards require carrier-specific APN configuration and network registration before SMS functionality becomes available.

## Known Limitations

- **Delivery Confirmation Scope**: Only the final segment of multipart SMS messages receives delivery confirmation tracking, which may mask delivery failures in earlier message parts
- **Sequential Processing**: Messages are processed sequentially, which ensures reliability but may impact throughput for high-volume scenarios
