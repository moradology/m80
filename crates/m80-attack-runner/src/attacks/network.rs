//! Network isolation attack attempts.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsRawFd;
use std::time::Duration;

use nix::libc;
use nix::sys::socket::{
    recv, send, setsockopt, socket, sockopt, AddressFamily, MsgFlags, SockFlag, SockProtocol,
    SockType,
};
use nix::sys::time::{TimeVal, TimeValLike};

use crate::{blocked, AttackResult};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const NETLINK_HEADER_LEN: usize = 16;
const IFINFO_MSG_LEN: usize = 16;
const RT_MSG_LEN: usize = 12;
const NETLINK_SEQ: u32 = 0x4d3830;

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

pub(crate) fn open_raw_socket() -> AttackResult {
    socket(
        AddressFamily::Inet,
        SockType::Raw,
        SockFlag::SOCK_CLOEXEC,
        SockProtocol::Raw,
    )
    .map(drop)
    .map_err(|err| blocked("socket(AF_INET, SOCK_RAW)", err))
}

pub(crate) fn raw_packet_inject() -> AttackResult {
    socket(
        AddressFamily::Packet,
        SockType::Raw,
        SockFlag::SOCK_CLOEXEC,
        SockProtocol::EthAll,
    )
    .map(drop)
    .map_err(|err| blocked("socket(AF_PACKET, SOCK_RAW)", err))
}

pub(crate) fn send_arbitrary_netlink() -> AttackResult {
    netlink_mutation(libc::RTM_NEWLINK, &ifinfo_payload(), "RTM_NEWLINK mutation")
}

pub(crate) fn bind_on_host_interface() -> AttackResult {
    UdpSocket::bind((Ipv4Addr::new(203, 0, 113, 10), 0))
        .map(|_| ())
        .map_err(|err| blocked("bind nonlocal TEST-NET-3 address", err))
}

pub(crate) fn privileged_route_mutation() -> AttackResult {
    netlink_mutation(
        libc::RTM_NEWROUTE,
        &route_payload(),
        "RTM_NEWROUTE mutation",
    )
}

fn connect_v4(addr: Ipv4Addr, port: u16, label: &'static str) -> AttackResult {
    let socket = SocketAddr::from((addr, port));
    TcpStream::connect_timeout(&socket, CONNECT_TIMEOUT)
        .map(|_| ())
        .map_err(|err| blocked(format!("connect {label} {socket}"), err))
}

fn netlink_mutation(message_type: u16, payload: &[u8], label: &'static str) -> AttackResult {
    let fd = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::SOCK_CLOEXEC,
        SockProtocol::NetlinkRoute,
    )
    .map_err(|err| blocked(format!("socket NETLINK_ROUTE for {label}"), err))?;
    setsockopt(&fd, sockopt::ReceiveTimeout, &TimeVal::milliseconds(200))
        .map_err(|err| blocked(format!("set {label} receive timeout"), err))?;

    let request = netlink_request(message_type, payload);
    send(fd.as_raw_fd(), &request, MsgFlags::empty())
        .map_err(|err| blocked(format!("send {label}"), err))?;

    let mut response = [0_u8; 256];
    let len = recv(fd.as_raw_fd(), &mut response, MsgFlags::empty())
        .map_err(|err| blocked(format!("receive {label} ack"), err))?;
    if len < NETLINK_HEADER_LEN + 4 {
        return Err(crate::AttackBlocked::new(format!(
            "{label}: short netlink response ({len} bytes)"
        )));
    }

    let message = read_u16(&response[4..6]);
    if message != libc::NLMSG_ERROR as u16 {
        return Err(crate::AttackBlocked::new(format!(
            "{label}: unexpected netlink response type {message}"
        )));
    }

    let error = read_i32(&response[NETLINK_HEADER_LEN..NETLINK_HEADER_LEN + 4]);
    if error == 0 {
        Ok(())
    } else {
        Err(crate::AttackBlocked::new(format!(
            "{label}: kernel rejected mutation with {}",
            std::io::Error::from_raw_os_error(-error)
        )))
    }
}

fn netlink_request(message_type: u16, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(NETLINK_HEADER_LEN + payload.len()).expect("netlink payload fits u32");
    let flags =
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK | libc::NLM_F_CREATE | libc::NLM_F_EXCL) as u16;
    let mut request = Vec::with_capacity(len as usize);
    request.extend_from_slice(&len.to_ne_bytes());
    request.extend_from_slice(&message_type.to_ne_bytes());
    request.extend_from_slice(&flags.to_ne_bytes());
    request.extend_from_slice(&NETLINK_SEQ.to_ne_bytes());
    request.extend_from_slice(&0_u32.to_ne_bytes());
    request.extend_from_slice(payload);
    request
}

fn ifinfo_payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(IFINFO_MSG_LEN);
    payload.push(libc::AF_UNSPEC as u8);
    payload.push(0);
    payload.extend_from_slice(&0_u16.to_ne_bytes());
    payload.extend_from_slice(&1_i32.to_ne_bytes());
    payload.extend_from_slice(&0_u32.to_ne_bytes());
    payload.extend_from_slice(&0_u32.to_ne_bytes());
    payload
}

fn route_payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(RT_MSG_LEN);
    payload.push(libc::AF_INET as u8);
    payload.push(0);
    payload.push(0);
    payload.push(0);
    payload.push(libc::RT_TABLE_MAIN);
    payload.push(libc::RTPROT_STATIC);
    payload.push(libc::RT_SCOPE_UNIVERSE);
    payload.push(libc::RTN_UNICAST);
    payload.extend_from_slice(&0_u32.to_ne_bytes());
    payload
}

fn read_u16(bytes: &[u8]) -> u16 {
    let mut value = [0_u8; 2];
    value.copy_from_slice(bytes);
    u16::from_ne_bytes(value)
}

fn read_i32(bytes: &[u8]) -> i32 {
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    i32::from_ne_bytes(value)
}
