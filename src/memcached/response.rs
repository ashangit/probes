use std::io::Cursor;

use bytes::Buf;

use crate::memcached::header::ResponseHeader;
use crate::memcached::{MemcachedError, MemcachedErrorKind};

pub struct Response {
    pub header: ResponseHeader,
}

impl Response {
    /// Check buffer has enough bytes to process the  response
    /// has enough bytes to process the header response
    ///
    /// # Arguments
    ///
    /// * `src` - buffer of bytes
    ///
    /// # Return
    ///
    /// * The size of bytes to read for the response
    ///   or an incomplete error if there are not enough bytes from the buffer
    ///   or an Other error from response header check
    ///
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<usize, MemcachedError> {
        let total_len = ResponseHeader::check(src)?;

        // Check remaining
        if src.remaining() < total_len {
            return Err(MemcachedErrorKind::Incomplete.into());
        }

        Ok(total_len)
    }
    /// Create response from buffer of bytes
    ///
    /// # Arguments
    ///
    /// * `src` - buffer of bytes
    ///
    /// # Return
    ///
    /// * Response
    ///
    pub fn parse(src: &mut Cursor<&[u8]>) -> Response {
        let header = ResponseHeader::parse(src);
        Response { header }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::memcached::response::Response;
    use crate::memcached::{MemcachedError, MemcachedErrorKind};

    fn check(input: &str) -> Result<usize, MemcachedError> {
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
        assert_eq!(
            res.err().unwrap(),
            MemcachedError(MemcachedErrorKind::Incomplete)
        );
    }

    #[test]
    fn parse_response() {
        let decoded =
            hex::decode("81000000040000000000000c00000000000000000000000100000000546573744e69636f")
                .expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let response = Response::parse(&mut cursor);
        assert_eq!(response.header.total_body_length, 12);
    }
}
