//! The three body-cap regimes, applied to decoded bytes.
//!
//! `Some(0)` never touches the body, `Some(n)` keeps the first `n` bytes and
//! notes when more followed, and `None` reads everything up to the root
//! ceiling, past which the read stops with the same note. Truncation is never
//! an error: a check evaluates what it received, with the flag as evidence.
//! Bytes are counted after decompression, so a compression bomb stops at the
//! ceiling too.

use std::io::{self, Read};

/// Cap for probes that read no body at all.
pub const STATUS_ONLY_BODY_BYTES: usize = 0;
/// Cap for probes that inspect a short body: JSON errors, twin text, cards.
pub const AUDIT_PROBE_MAX_BODY_BYTES: usize = 64 * 1024;
/// Ceiling on an uncapped read. The site reads the root document in full
/// within Workers memory; the local engine draws a line here so a huge page
/// cannot exhaust the host.
pub const AUDIT_ROOT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

const CHUNK: usize = 16 * 1024;

/// What a capped read produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyRead {
    /// The bytes kept.
    pub bytes: Vec<u8>,
    /// More bytes followed the cap and were dropped.
    pub truncated: bool,
}

/// Read under a cap. `None` means the root ceiling.
pub fn read_capped<R: Read>(mut reader: R, cap: Option<usize>) -> io::Result<BodyRead> {
    let limit = match cap {
        Some(STATUS_ONLY_BODY_BYTES) => return Ok(BodyRead::default()),
        Some(n) => n,
        None => AUDIT_ROOT_MAX_BODY_BYTES,
    };
    let mut bytes = Vec::new();
    let mut buf = [0u8; CHUNK];
    while bytes.len() < limit {
        let want = (limit - bytes.len()).min(CHUNK);
        let n = read_some(&mut reader, &mut buf[..want])?;
        if n == 0 {
            return Ok(BodyRead {
                bytes,
                truncated: false,
            });
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    let mut probe = [0u8; 1];
    let truncated = read_some(&mut reader, &mut probe)? > 0;
    Ok(BodyRead { bytes, truncated })
}

fn read_some<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn a_zero_cap_reads_nothing() {
        let read = read_capped(Cursor::new(vec![b'x'; 10]), Some(STATUS_ONLY_BODY_BYTES)).unwrap();
        assert!(read.bytes.is_empty());
        assert!(!read.truncated);
    }

    #[test]
    fn a_body_under_the_cap_is_whole_and_unflagged() {
        let read = read_capped(Cursor::new(b"hello".to_vec()), Some(16)).unwrap();
        assert_eq!(read.bytes, b"hello");
        assert!(!read.truncated);
    }

    #[test]
    fn a_body_at_the_cap_is_whole_and_unflagged() {
        let read = read_capped(Cursor::new(vec![b'x'; 16]), Some(16)).unwrap();
        assert_eq!(read.bytes.len(), 16);
        assert!(!read.truncated);
    }

    #[test]
    fn a_body_over_the_cap_is_cut_and_flagged() {
        let read = read_capped(Cursor::new(vec![b'x'; 17]), Some(16)).unwrap();
        assert_eq!(read.bytes.len(), 16);
        assert!(read.truncated);
    }

    #[test]
    fn an_uncapped_read_stops_at_the_root_ceiling() {
        let body = io::repeat(b'z').take(AUDIT_ROOT_MAX_BODY_BYTES as u64 + 5);
        let read = read_capped(body, None).unwrap();
        assert_eq!(read.bytes.len(), AUDIT_ROOT_MAX_BODY_BYTES);
        assert!(read.truncated);
    }

    #[test]
    fn a_read_error_is_returned_not_swallowed() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("connection reset"))
            }
        }
        let err = read_capped(Broken, Some(16)).unwrap_err();
        assert_eq!(err.to_string(), "connection reset");
    }
}
