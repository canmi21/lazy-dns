# Lazy DNS

A lightweight, plug-and-play DNS server with auto-reload configuration and GeoIP-based routing capabilities. Designed for simplicity, Lazy DNS supports only A, AAAA, and CNAME records, intentionally dropping all other record types.

## Features

- **Simple Configuration**: Define DNS records in a TOML file with support for A, AAAA, and CNAME records.
- **GeoIP Routing**: Route DNS queries based on the client's country using an external GeoIP service (`lazy-mmdb`).
- **Load Balancing**: Randomly select a single record from multiple A, AAAA, or CNAME entries for basic load balancing.
- **Auto-Reload Config**: Automatically loads configuration from a specified path or defaults to `~/lazy-dns/config.toml`.
- **Lightweight and Fast**: Built with Rust and Tokio for high performance and low resource usage.

## Installation

### Prerequisites

- **Rust**: Ensure you have Rust installed (version 1.65 or later recommended). Install via [rustup](https://rustup.rs/).
- **GeoIP Service (Optional)**: For GeoIP routing, a `lazy-mmdb` service must be running and accessible via a Unix socket at `/tmp/lazy-mmdb.sock`.

### Steps

1. Clone the repository:
   ```bash
   git clone https://github.com/canmi21/lazy-dns.git
   cd lazy-dns
   ```

2. Build and run the project:
   ```bash
   cargo build --release
   cargo run --release
   ```

3. (Optional) Install the binary:
   ```bash
   cargo install --path .
   ```

   This makes the `lazy-dns` command available globally.

## Configuration

Lazy DNS uses a TOML configuration file to define DNS records and settings. By default, it looks for `~/lazy-dns/config.toml`. You can override this by setting the `CONFIG_PATH` environment variable.

### Example Configuration

The default configuration is created automatically if no config file is found. Below is an example from `.env.example` and the default `config.toml`:

```toml
# Default TTL for all records in minutes, if not specified per domain.
default_ttl = 5

[domains]

# Simple domain with A and AAAA records.
[domains."test.local"]
a = ["127.0.0.1"]
aaaa = ["::1"]

# Domain with multiple A records for random selection (load balancing).
[domains."roundrobin.local"]
a = ["192.168.1.10", "192.168.1.20"]
ttl = 1

# Domain with GeoIP routing rules.
[domains."geo.local"]
a = ["8.8.8.8"]
cname = ["default.geo.local"]
[domains."geo.local".country]
US = { a = ["1.1.1.1", "1.0.0.1"], cname = ["us.geo.local"] }
CN = { a = ["114.114.114.114"], aaaa = ["2400:3200::1"] }
JP = { cname = ["jp.geo.local"] }
```

### Environment Variables

Configuration can be customized via environment variables, as shown in `.env.example`:

- `LOG_LEVEL`: Set logging verbosity (`debug`, `info`, `warn`, `error`). Default: `info`.
- `BIND_PORT`: Port for the DNS server. Default: `53` (requires root privileges for ports < 1024).
- `CONFIG_PATH`: Path to the TOML config file. Default: `~/lazy-dns/config.toml`.
- `GEOIP_RECONNECT_SECONDS`: Interval to retry connecting to the GeoIP service. Default: `300` seconds.

Example `.env` file:
```bash
LOG_LEVEL=debug
BIND_PORT=5353
CONFIG_PATH=/path/to/custom/config.toml
GEOIP_RECONNECT_SECONDS=600
```

## Usage

1. Ensure the configuration file is set up as needed.
2. Start the DNS server:
   ```bash
   lazy-dns
   ```
   Or specify a custom port:
   ```bash
   BIND_PORT=5353 lazy-dns
   ```

3. Test the server using a DNS client like `dig`:
   ```bash
   dig @127.0.0.1 -p 5353 test.local
   ```

4. For GeoIP routing, ensure the `lazy-mmdb` service is running and accessible at `/tmp/lazy-mmdb.sock`.

## Project Structure

```plaintext
lazy-dns/
├── src/
│   ├── config.rs        # Configuration loading and parsing
│   ├── dns_server.rs    # DNS server implementation
│   ├── geoip.rs         # GeoIP client for country-based routing
│   ├── main.rs          # Entry point
│   ├── resolver.rs      # DNS query resolution logic
├── .env.example         # Example environment variables
├── Cargo.toml           # Rust project configuration
├── LICENSE              # MIT License
```

## Dependencies

Lazy DNS relies on the following Rust crates:
- `dotenvy`: For environment variable parsing.
- `fancy-log`: For colorful and leveled logging.
- `hickory-proto`: For DNS protocol handling.
- `lazy-motd`: For a startup message.
- `tokio`: For asynchronous runtime.
- `serde` and `toml`: For configuration parsing.
- `rand`: For random record selection (load balancing).
- `dirs`: For finding the home directory.
- `serde_json`: For GeoIP response parsing.
- `parking_lot`: For thread-safe locking.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on the [GitHub repository](https://github.com/canmi21/lazy-dns).

## Contact

For questions or feedback, open an issue on the GitHub repository or contact the maintainer at the repository's listed contact information.