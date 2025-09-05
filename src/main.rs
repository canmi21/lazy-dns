/* src/main.rs */

use dotenvy::dotenv;
use fancy_log::{LogLevel, log, set_log_level};
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use lazy_motd::lazy_motd;
use std::env;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let level = env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "info".to_string())
        .to_lowercase();
    let log_level = match level.as_str() {
        "debug" => LogLevel::Debug,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    };
    set_log_level(log_level);
    lazy_motd!();

    let port = env::var("BIND_PORT").unwrap_or_else(|_| "53".to_string());
    let bind_addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&bind_addr).await?;

    log(
        LogLevel::Info,
        &format!("Lazy DNS server started on localhost:{}", port),
    );

    let mut buf = [0; 512];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                log(
                    LogLevel::Debug,
                    &format!("Received {} bytes from {}", len, addr),
                );

                match Message::from_bytes(&buf[..len]) {
                    Ok(request) => {
                        log(
                            LogLevel::Debug,
                            &format!("Parsed DNS request: ID={}", request.id()),
                        );

                        if let Some(response_bytes) = handle_dns_request(request).await {
                            if let Err(e) = socket.send_to(&response_bytes, addr).await {
                                log(LogLevel::Error, &format!("Failed to send response: {}", e));
                            } else {
                                log(LogLevel::Debug, &format!("Sent response to {}", addr));
                            }
                        }
                    }
                    Err(e) => {
                        log(
                            LogLevel::Warn,
                            &format!("Failed to parse DNS request: {}", e),
                        );
                    }
                }
            }
            Err(e) => {
                log(LogLevel::Error, &format!("Failed to receive data: {}", e));
            }
        }
    }
}

async fn handle_dns_request(request: Message) -> Option<Vec<u8>> {
    if request.message_type() != MessageType::Query || request.op_code() != OpCode::Query {
        log(LogLevel::Debug, "Not a standard query, dropping");
        return None;
    }

    let queries = request.queries();
    if queries.is_empty() {
        log(LogLevel::Debug, "No queries in request, dropping");
        return None;
    }

    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_authoritative(false);
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    response.set_response_code(ResponseCode::NoError);

    for query in queries {
        response.add_query(query.clone());

        if query.query_type() == RecordType::A {
            log(
                LogLevel::Info,
                &format!("Processing A record query for: {}", query.name()),
            );

            let a_record = A::new(127, 0, 0, 1);
            let record = Record::from_rdata(
                query.name().clone(),
                300, // TTL 5分钟
                RData::A(a_record),
            );

            response.add_answer(record);
            log(
                LogLevel::Info,
                &format!("Added A record: {} -> 127.0.0.1", query.name()),
            );
        } else {
            log(
                LogLevel::Debug,
                &format!("Unsupported query type: {:?}, dropping", query.query_type()),
            );
            response.set_response_code(ResponseCode::NotImp);
        }
    }

    match response.to_bytes() {
        Ok(bytes) => {
            log(
                LogLevel::Debug,
                &format!("Response serialized, {} bytes", bytes.len()),
            );
            Some(bytes)
        }
        Err(e) => {
            log(
                LogLevel::Error,
                &format!("Failed to serialize response: {}", e),
            );
            None
        }
    }
}
