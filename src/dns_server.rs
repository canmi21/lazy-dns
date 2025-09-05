/* src/dns_server.rs */

use crate::resolver::DnsResolver;
use fancy_log::{LogLevel, log};
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

pub async fn run_server(
    bind_addr: &str,
    resolver: Arc<DnsResolver>,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
    let mut buf = [0; 512];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        let socket = socket.clone();
        let resolver = resolver.clone();
        let request_data = buf[..len].to_vec();

        tokio::spawn(async move {
            if let Some(response_bytes) = handle_request(request_data, addr, resolver).await {
                if let Err(e) = socket.send_to(&response_bytes, addr).await {
                    log(
                        LogLevel::Error,
                        &format!("Failed to send response to {}: {}", addr, e),
                    );
                }
            }
        });
    }
}

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
    response.set_authoritative(true); // We are authoritative for our zones.

    if let Some(query) = request.queries().first() {
        let answers = resolver.resolve(query, addr.ip()).await;
        if answers.is_empty() {
            response.set_response_code(ResponseCode::NXDomain); // Domain not found
        } else {
            for answer in answers {
                response.add_answer(answer);
            }
            response.set_response_code(ResponseCode::NoError);
        }
    } else {
        response.set_response_code(ResponseCode::FormErr); // No queries in request
    }

    match response.to_bytes() {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            log(
                LogLevel::Error,
                &format!("Failed to serialize response: {}", e),
            );
            None
        }
    }
}
