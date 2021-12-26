use std::io::Cursor;

use bytes::{Buf, Bytes};
use log::debug;

use crate::memcached::header::{RequestHeader, ResponseHeader};

pub struct FrameRequest {
    header: RequestHeader,
    extra: Bytes,
    key: Bytes,
    value: Bytes,
}
pub struct FrameResponse {
    header: ResponseHeader,
    extra: Bytes,
    key: Bytes,
    value: Bytes,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    Incomplete,
    Other,
}

impl FrameResponse {
    /// Check buffer has enough bytes to process the  response
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<(), Error> {
        let total_len = match ResponseHeader::check(src) {
            Err(issue) => {
                return Err(issue);
            }
            Ok(_total_len) => _total_len,
        };

        // Check remaining
        if src.remaining() < total_len {
            return Err(Error::Incomplete);
        }

        src.advance(total_len);

        Ok(())
    }

    pub fn parse(src: &mut Cursor<&[u8]>) -> FrameResponse {
        let header = ResponseHeader::parse(src);
        let extra = src.copy_to_bytes(header.extra_length as usize);
        let key = src.copy_to_bytes(header.key_length as usize);
        let value = src.copy_to_bytes(
            header.total_body_length as usize
                - header.key_length as usize
                - header.extra_length as usize,
        );
        FrameResponse {
            header,
            extra,
            key,
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bytes::Bytes;

    use crate::memcached::frame::{Error, FrameResponse};

    fn check(input: &str) -> (Result<(), Error>, u64) {
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        (FrameResponse::check(&mut cursor), cursor.position())
    }

    #[test]
    fn check_frame() {
        let (res, position) = check("8100000004000000000000050000000000000000000000010000000030");
        assert!(res.is_ok());
        assert_eq!(position, 29);
    }

    #[test]
    fn check_response_header_incomplete() {
        let (res, _) = check("8100000004000000000000100000000000000000000000010000000030");
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::Incomplete);
    }

    #[test]
    fn parse_frame() {
        let decoded =
            hex::decode("81000000040000000000000c00000000000000000000000100000000546573744e69636f")
                .expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let response = FrameResponse::parse(&mut cursor);
        assert_eq!(response.header.total_body_length, 12);
        assert_eq!(response.extra, Bytes::from_static(b"\0\0\0\0"));
        assert_eq!(response.key, Bytes::from_static(b""));
        assert_eq!(response.value, Bytes::from_static(b"TestNico"));
    }
}
