//! Network isolation attack attempts.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use crate::{blocked, AttackResult};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn connect_imds_http() -> AttackResult {
    connect_v4(Ipv4Addr::new(169, 254, 169, 254), 80, "IMDS HTTP")
}

pub(crate) fn connect_public_dns_tcp() -> AttackResult {
    connect_v4(Ipv4Addr::new(1, 1, 1, 1), 53, "public DNS TCP")
}

pub(crate) fn connect_private_rfc1918() -> AttackResult {
    connect_v4(Ipv4Addr::new(10, 0, 0, 1), 80, "private RFC1918")
}

pub(crate) fn bind_privileged_port() -> AttackResult {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, 80))
        .map(|_| ())
        .map_err(|err| blocked("bind privileged port 80", err))
}

pub(crate) fn listen_all_interfaces() -> AttackResult {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
        .map(|_| ())
        .map_err(|err| blocked("bind 0.0.0.0:0", err))
}

fn connect_v4(addr: Ipv4Addr, port: u16, label: &'static str) -> AttackResult {
    let socket = SocketAddr::from((addr, port));
    TcpStream::connect_timeout(&socket, CONNECT_TIMEOUT)
        .map(|_| ())
        .map_err(|err| blocked(format!("connect {label} {socket}"), err))
}
