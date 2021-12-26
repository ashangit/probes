use std::io::Cursor;

use bytes::{Buf, Bytes};

use crate::memcached::frame::Error;

// https://www.slideshare.net/tmaesaka/memcached-binary-protocol-in-a-nutshell-presentation
pub struct RequestHeader {
    magic: Bytes,
    opcode: Bytes,
    key_length: Bytes,
    extra_length: Bytes,
    data_type: Bytes,
    reserved: Bytes,
    total_body_length: Bytes,
    opaque: Bytes,
    cas: Bytes,
}

impl RequestHeader {
    pub fn new(
        magic: Bytes,
        opcode: Bytes,
        key_length: Bytes,
        extra_length: Bytes,
        data_type: Bytes,
        reserved: Bytes,
        total_body_length: Bytes,
        opaque: Bytes,
        cas: Bytes,
    ) -> RequestHeader {
        RequestHeader {
            magic,
            opcode,
            key_length,
            extra_length,
            data_type,
            reserved,
            total_body_length,
            opaque,
            cas,
        }
    }
}

pub static RESPONSE_HEADER_SIZE: u8 = 24;

#[derive(Debug, PartialEq)]
pub struct ResponseHeader {
    magic: Bytes,
    opcode: Bytes,
    pub(crate) key_length: u16,
    pub(crate) extra_length: u8,
    data_type: Bytes,
    status: Bytes,
    pub(crate) total_body_length: u32,
    opaque: Bytes,
    cas: Bytes,
}

impl ResponseHeader {
    pub(crate) fn parse(src: &mut Cursor<&[u8]>) -> ResponseHeader {
        ResponseHeader {
            magic: src.copy_to_bytes(1),
            opcode: src.copy_to_bytes(1),
            key_length: src.get_u16(),
            extra_length: src.get_u8(),
            data_type: src.copy_to_bytes(1),
            status: src.copy_to_bytes(2),
            total_body_length: src.get_u32(),
            opaque: src.copy_to_bytes(4),
            cas: src.copy_to_bytes(8),
        }
    }

    /// Check buffer contains magic field x81 and
    /// has enough bytes to process the header response
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<usize, Error> {
        // Check enough bytes to read for a response header
        if src.remaining() < RESPONSE_HEADER_SIZE as usize {
            return Err(Error::Incomplete);
        }

        // CHeck magic field is the one forResponse Packet
        if src.copy_to_bytes(1) != Bytes::from_static(b"\x81") {
            return Err(Error::Other);
        }

        // Read body length field to compute total len response
        src.advance(7);
        let total_body_size = src.get_u32();
        let total_len: usize = (RESPONSE_HEADER_SIZE as u32 + total_body_size) as usize;

        // Reset position
        src.set_position(0);

        Ok(total_len)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bytes::Bytes;

    use crate::memcached::frame::Error;
    use crate::memcached::header::ResponseHeader;

    #[test]
    fn parse_response_header() {
        let input = "8100000004000000000000050000000000000000000000010000000030";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::parse(&mut cursor);
        let response = ResponseHeader {
            magic: Bytes::from_static(b"\x81"),
            opcode: Bytes::from_static(b"\0"),
            key_length: 0,
            extra_length: 4,
            data_type: Bytes::from_static(b"\0"),
            status: Bytes::from_static(b"\0\0"),
            total_body_length: 5,
            opaque: Bytes::from_static(b"\0\0\0\0"),
            cas: Bytes::from_static(b"\0\0\0\0\0\0\0\x01"),
        };
        assert_eq!(res, response);
    }

    #[test]
    fn check_response_header() {
        let input = "8100000004000000000000050000000000000000000000010000000030";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::check(&mut cursor)
            .ok()
            .expect("Failed to get total length response");
        assert_eq!(res, 29);
    }

    #[test]
    fn check_response_header_bad_magic() {
        let input = "8000000004000000000000050000000000000000000000010000000030";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::check(&mut cursor);
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::Other);
    }

    #[test]
    fn check_response_header_incomplete() {
        let input = "81000000040000000000000500000000000000000000";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::check(&mut cursor);
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), Error::Incomplete);
    }
}
