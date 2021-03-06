use alvr_common::{data::*, logging::*, *};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use tokio::net::*;

const LOCAL_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const MAX_HANDSHAKE_PACKET_SIZE_BYTES: usize = 4_000;
const HANDSHAKE_PORT: u16 = 9943;

pub async fn search_client(
    client_ip: Option<String>,
    client_found_cb: impl Fn(IpAddr, ClientHandshakePacket) -> Option<ServerHandshakePacket>,
) -> StrResult {
    let mut handshake_socket =
        trace_err!(UdpSocket::bind(SocketAddr::new(LOCAL_IP, HANDSHAKE_PORT)).await)?;

    let maybe_target_client_ip = match client_ip {
        Some(ip_str) => Some(trace_err!(ip_str.parse::<IpAddr>(), "Client IP")?),
        None => None,
    };

    let mut packet_buffer = [0u8; MAX_HANDSHAKE_PACKET_SIZE_BYTES];

    loop {
        let (hanshake_packet_size, address) =
            match handshake_socket.recv_from(&mut packet_buffer).await {
                Ok(pair) => pair,
                Err(e) => {
                    debug!("Error receiving handshake packet: {}", e);
                    continue;
                }
            };

        if let Some(ip) = maybe_target_client_ip {
            if address.ip() != ip {
                info!(id: LogId::ClientFoundWrongIp);
                continue;
            }
        }

        let client_handshake_packet: ClientHandshakePacket =
            match bincode::deserialize(&packet_buffer[..hanshake_packet_size]) {
                Ok(client_handshake_packet) => client_handshake_packet,
                Err(e) => {
                    warn!(
                        id: LogId::ClientFoundInvalid,
                        "Received handshake packet: {}", e
                    );
                    continue;
                }
            };

        if client_handshake_packet.alvr_name != [b'A', b'L', b'V', b'R'] {
            warn!(
                id: LogId::ClientFoundInvalid,
                "Received handshake packet: wrong name"
            );
            continue;
        }

        let version = {
            let nul_range_end = client_handshake_packet
                .version
                .iter()
                .position(|&c| c == b'\0')
                .unwrap_or_else(|| client_handshake_packet.version.len());
            String::from_utf8_lossy(&client_handshake_packet.version[0..nul_range_end])
        };

        match is_version_compatible(&version, ALVR_CLIENT_VERSION_REQ) {
            Ok(compatible) => {
                if !compatible {
                    warn!(id: LogId::ClientFoundWrongVersion(version.into()));
                    continue;
                }
            }
            Err(e) => {
                warn!(
                    id: LogId::ClientFoundInvalid,
                    "Received handshake packet: {}", e
                );
                continue;
            }
        }

        let maybe_server_handshake_packet =
            client_found_cb(address.ip(), client_handshake_packet);

        if let Some(server_handshake_packet) = maybe_server_handshake_packet {
            let packet = trace_err!(bincode::serialize(&server_handshake_packet))?;
            handshake_socket
                .send_to(&packet, SocketAddr::new(address.ip(), 9944))
                .await
                .ok();
        }
    }
}
