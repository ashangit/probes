use std::io::Cursor;

use bytes::{Buf, Bytes};

use crate::memcached::header::ResponseHeader;

// pub struct FrameRequest {
//     header: RequestHeader,
//     extra: Bytes,
//     key: Bytes,
//     value: Bytes,
// }
pub struct Response {
    pub header: ResponseHeader,
    extra: Bytes,
    key: Bytes,
    value: Bytes,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    Incomplete,
    Other,
}

impl Response {
    /// Check buffer has enough bytes to process the  response
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<usize, Error> {
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

        Ok(total_len)
    }

    pub fn parse(src: &mut Cursor<&[u8]>) -> Response {
        let header = ResponseHeader::parse(src);
        let extra = src.copy_to_bytes(header.extra_length as usize);
        let key = src.copy_to_bytes(header.key_length as usize);
        let value = src.copy_to_bytes(
            header.total_body_length as usize
                - header.key_length as usize
                - header.extra_length as usize,
        );
        Response {
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

    use crate::memcached::response::{Error, Response};

    fn check(input: &str) -> Result<usize, Error> {
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        Response::check(&mut cursor)
    }

    #[test]
    fn check_response() {
        let res = check("8100000004000000000000050000000000000000000000010000000030");
        assert!(res.is_ok());
        assert_eq!(res.ok().unwrap(), 29);
    }

    #[test]
    fn check_response_header_incomplete() {
        let res = check("8100000004000000000000100000000000000000000000010000000030");
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::Incomplete);
    }

    #[test]
    fn parse_response() {
        let decoded =
            hex::decode("81000000040000000000000c00000000000000000000000100000000546573744e69636f")
                .expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let response = Response::parse(&mut cursor);
        assert_eq!(response.header.total_body_length, 12);
        assert_eq!(response.extra, Bytes::from_static(b"\0\0\0\0"));
        assert_eq!(response.key, Bytes::from_static(b""));
        assert_eq!(response.value, Bytes::from_static(b"TestNico"));
    }
}
