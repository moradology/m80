use std::io::{Read, Write};
use std::path::Path;

use m80_proto::{FileError, FileReadChunk, FileReadRequest, FILE_READ_LIMIT_DEFAULT};

use super::{map_io_error, open_nofollow_read, respond};

const FILE_READ_CHUNK_BYTES: usize = 1024 * 1024;

pub(super) fn stream_file_read<W: Write>(
    writer: &mut W,
    request_id: Option<String>,
    req: FileReadRequest,
) -> anyhow::Result<()> {
    let path = Path::new(&req.path);
    let limit = req
        .max_bytes
        .unwrap_or(FILE_READ_LIMIT_DEFAULT)
        .min(usize::MAX as u64) as usize;

    let mut file = match open_nofollow_read(path) {
        Ok(file) => file,
        Err(error) => {
            respond(
                writer,
                request_id,
                file_read_terminal(0, false, Some(error)),
            )?;
            return Ok(());
        }
    };

    let mut seq = 0u64;
    let mut remaining = limit;
    let mut buf = vec![0u8; FILE_READ_CHUNK_BYTES];
    while remaining > 0 {
        let n = match file.read(&mut buf[..remaining.min(FILE_READ_CHUNK_BYTES)]) {
            Ok(0) => {
                respond(writer, request_id, file_read_terminal(seq, false, None))?;
                return Ok(());
            }
            Ok(n) => n,
            Err(e) => {
                respond(
                    writer,
                    request_id,
                    file_read_terminal(seq, false, Some(map_io_error(&e))),
                )?;
                return Ok(());
            }
        };
        respond(
            writer,
            request_id.clone(),
            FileReadChunk {
                seq,
                bytes: buf[..n].to_vec(),
                done: false,
                truncated: false,
                error: None,
            },
        )?;
        seq = seq
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("file_read chunk sequence overflow"))?;
        remaining -= n;
    }

    let mut extra = [0u8; 1];
    let terminal = match file.read(&mut extra) {
        Ok(n) => file_read_terminal(seq, n > 0, None),
        Err(e) => file_read_terminal(seq, false, Some(map_io_error(&e))),
    };
    respond(writer, request_id, terminal)?;
    Ok(())
}

fn file_read_terminal(seq: u64, truncated: bool, error: Option<FileError>) -> FileReadChunk {
    FileReadChunk {
        seq,
        bytes: Vec::new(),
        done: true,
        truncated,
        error,
    }
}
