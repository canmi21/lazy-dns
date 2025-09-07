/* src/dns_server.rs */

use crate::config::UnconfiguredPolicy;
use crate::resolver::DnsResolver;
use fancy_log::{LogLevel, log};
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// Runs both the UDP and TCP DNS servers concurrently.
pub async fn run_server(
    bind_addr: &str,
    resolver: Arc<DnsResolver>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bind both UDP and TCP listeners to the same address
    let udp_socket = Arc::new(UdpSocket::bind(bind_addr).await?);
    let tcp_listener = TcpListener::bind(bind_addr).await?;

    log(
        LogLevel::Info,
        &format!("DNS server listening for UDP and TCP on {}", bind_addr),
    );

    // Buffer for incoming UDP packets
    let mut udp_buf = [0; 512];

    loop {
        // Use tokio::select! to handle the first available event from either socket
        tokio::select! {
            // Handle incoming UDP packets
            Ok((len, addr)) = udp_socket.recv_from(&mut udp_buf) => {
                let data = udp_buf[..len].to_vec();
                let resolver_clone = resolver.clone();
                let udp_socket_clone = udp_socket.clone();

                tokio::spawn(async move {
                    if let Some(response_bytes) = handle_request(data, addr, resolver_clone).await {
                        if let Err(e) = udp_socket_clone.send_to(&response_bytes, addr).await {
                            log(LogLevel::Error, &format!("Failed to send UDP response to {}: {}", addr, e));
                        }
                    }
                });
            },

            // Handle incoming TCP connections
            Ok((stream, addr)) = tcp_listener.accept() => {
                let resolver_clone = resolver.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_connection(stream, addr, resolver_clone).await {
                        log(LogLevel::Warn, &format!("TCP connection error from {}: {}", addr, e));
                    }
                });
            },
        }
    }
}

/// Handles a single TCP connection, including message framing.
async fn handle_tcp_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    resolver: Arc<DnsResolver>,
) -> io::Result<()> {
    // DNS over TCP messages are prefixed with a 2-byte length field
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    // Read the full DNS query message
    let mut req_buf = vec![0u8; len];
    stream.read_exact(&mut req_buf).await?;

    // Process the request using the same shared handler
    if let Some(res_buf) = handle_request(req_buf, addr, resolver).await {
        // Prepend the response with its 2-byte length and send it back
        let res_len = res_buf.len() as u16;
        stream.write_all(&res_len.to_be_bytes()).await?;
        stream.write_all(&res_buf).await?;
    }
    // The stream is dropped here, closing the connection.
    Ok(())
}

/// The core request handler, protocol-agnostic.
async fn handle_request(
    data: Vec<u8>,
    addr: SocketAddr,
    resolver: Arc<DnsResolver>,
) -> Option<Vec<u8>> {
    let request = match Message::from_bytes(&data) {
        Ok(req) => req,
        Err(e) => {
            log(
                LogLevel::Warn,
                &format!("Failed to parse request from {}: {}", addr, e),
            );
            return None;
        }
    };

    if request.message_type() != MessageType::Query || request.op_code() != OpCode::Query {
        return None;
    }

    let mut response = Message::from(request.clone());
    response.set_message_type(MessageType::Response);
    response.set_authoritative(true);

    let query = match request.queries().first() {
        Some(q) => q,
        None => {
            response.set_response_code(ResponseCode::FormErr);
            return response.to_bytes().ok();
        }
    };

    let answers = resolver.resolve(query, addr.ip()).await;

    if answers.is_empty() {
        match resolver.config().unconfigured_policy {
            UnconfiguredPolicy::Drop => {
                return None;
            }
            UnconfiguredPolicy::Refused => {
                response.set_response_code(ResponseCode::Refused);
            }
            UnconfiguredPolicy::NxDomain => {
                if query.query_type() == RecordType::SOA {
                    response.set_response_code(ResponseCode::NoError);
                } else {
                    response.set_response_code(ResponseCode::NXDomain);
                }
            }
        }
        log(
            LogLevel::Info,
            &format!(
                "{} inquiry {} -> {}",
                addr.ip(),
                query.name(),
                response.response_code()
            ),
        );
    } else {
        let records_str = format_records(&answers);
        log(
            LogLevel::Info,
            &format!("{} inquiry {} get {}", addr.ip(), query.name(), records_str),
        );

        for answer in answers {
            response.add_answer(answer);
        }
        response.set_response_code(ResponseCode::NoError);
    }

    response.to_bytes().ok()
}

/// Helper function to format DNS records into a concise string for logging.
fn format_records(records: &[Record]) -> String {
    if records.is_empty() {
        return "[]".to_string();
    }

    let mut grouped = BTreeMap::<RecordType, Vec<String>>::new();

    for record in records {
        let rdata = record.data();
        let maybe_value = match rdata {
            RData::A(addr) => Some(addr.to_string()),
            RData::AAAA(addr) => Some(addr.to_string()),
            RData::CNAME(name) => Some(name.to_string().trim_end_matches('.').to_string()),
            RData::MX(mx) => Some(format!("{} {}", mx.preference(), mx.exchange())),
            RData::NS(name) => Some(name.to_string()),
            RData::SOA(soa) => Some(soa.mname().to_string()),
            RData::TXT(txt) => Some(txt.to_string()),
            _ => None, // For other record types, we produce nothing
        };

        if let Some(value) = maybe_value {
            grouped.entry(record.record_type()).or_default().push(value);
        }
    }

    if grouped.is_empty() {
        return "[]".to_string();
    }

    grouped
        .iter()
        .map(|(rtype, vals)| format!("{} [{}]", rtype, vals.join(", ")))
        .collect::<Vec<_>>()
        .join(" ")
}
