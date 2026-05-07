//! Host-connection liveness probing for cancel/control frames.

use std::io::BufReader;
use std::os::fd::{AsRawFd, RawFd};

use nix::errno::Errno;
use nix::sys::socket::{recv, MsgFlags};

/// Return true when a buffered reader already has bytes, when the socket has
/// bytes available, or when the peer has closed the connection.
pub(crate) fn borrowed_reader_has_data<R>(reader: &mut BufReader<&R>) -> bool
where
    R: AsRawFd,
{
    if !reader.buffer().is_empty() {
        return true;
    }
    fd_has_data_or_closed((*reader.get_ref()).as_raw_fd())
}

fn fd_has_data_or_closed(fd: RawFd) -> bool {
    let mut byte = [0u8; 1];
    match recv(fd, &mut byte, MsgFlags::MSG_PEEK | MsgFlags::MSG_DONTWAIT) {
        Ok(_) => true,
        Err(err) if err == Errno::EAGAIN || err == Errno::EWOULDBLOCK => false,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Read, Write};
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn open_socket_without_bytes_is_not_ready() {
        let (_writer, reader_stream) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(&reader_stream);

        assert!(!borrowed_reader_has_data(&mut reader));
    }

    #[test]
    fn socket_bytes_are_ready_without_consuming() {
        let (mut writer, reader_stream) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        writer.write_all(b"x").unwrap();

        assert!(borrowed_reader_has_data(&mut reader));

        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'x']);
    }

    #[test]
    fn buffered_bytes_are_ready_without_socket_peek() {
        let (mut writer, reader_stream) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        writer.write_all(b"x").unwrap();
        assert_eq!(reader.fill_buf().unwrap(), b"x");
        drop(writer);

        assert!(borrowed_reader_has_data(&mut reader));
    }

    #[test]
    fn peer_fin_is_ready_so_callers_observe_disconnect() {
        let (writer, reader_stream) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(&reader_stream);
        drop(writer);

        assert!(borrowed_reader_has_data(&mut reader));
        assert_eq!(reader.fill_buf().unwrap(), b"");
    }
}
