use std::io::Cursor;

use bytes::{Buf, Bytes};

use crate::memcached::{MemcachedError, MemcachedErrorKind};

const HEADER_SIZE: u8 = 24;

const REQUEST_PACKET: u8 = 128;
const DATA_TYPE: u8 = 0;
const RESERVED: u16 = 0;
const OPAQUE: u32 = 0;
const CAS: u64 = 0; // Not using cas inside the probe

// https://www.slideshare.net/tmaesaka/memcached-binary-protocol-in-a-nutshell-presentation
pub struct RequestHeader {
    magic: u8,
    opcode: u8,
    key_length: [u8; 2],
    extra_length: u8,
    data_type: u8,
    reserved: [u8; 2],
    total_body_length: [u8; 4],
    opaque: [u8; 4],
    cas: [u8; 8],
}

impl RequestHeader {
    pub fn new(opcode: u8, key_length: u16, extra_length: u8, value_length: u32) -> RequestHeader {
        let total_body_length: u32 = key_length as u32 + extra_length as u32 + value_length;
        RequestHeader {
            magic: REQUEST_PACKET,
            opcode,
            key_length: key_length.to_be_bytes(),
            extra_length,
            data_type: DATA_TYPE,
            reserved: RESERVED.to_be_bytes(),
            total_body_length: total_body_length.to_be_bytes(),
            opaque: OPAQUE.to_be_bytes(),
            cas: CAS.to_be_bytes(),
        }
    }

    pub fn as_bytes(&mut self) -> Vec<u8> {
        let mut request_bytes: Vec<u8> = Vec::with_capacity(HEADER_SIZE as usize);
        request_bytes.push(self.magic);
        request_bytes.push(self.opcode);
        request_bytes.extend(self.key_length);
        request_bytes.push(self.extra_length);
        request_bytes.push(self.data_type);
        request_bytes.extend(self.reserved);
        request_bytes.extend(self.total_body_length);
        request_bytes.extend(self.opaque);
        request_bytes.extend(self.cas);
        request_bytes
    }
}

#[derive(Debug, PartialEq)]
pub struct ResponseHeader {
    magic: Bytes,
    opcode: Bytes,
    pub(crate) key_length: u16,
    pub(crate) extra_length: u8,
    data_type: Bytes,
    pub status: Bytes,
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
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<usize, MemcachedError> {
        // Check enough bytes to read for a response header
        if src.remaining() < HEADER_SIZE as usize {
            return Err(MemcachedErrorKind::Incomplete.into());
        }

        // CHeck magic field is the one forResponse Packet
        if src.copy_to_bytes(1) != Bytes::from_static(b"\x81") {
            return Err(MemcachedErrorKind::Other.into());
        }

        // Read body length field to compute total len response
        src.advance(7);
        let total_body_size = src.get_u32();
        let total_len: usize = (HEADER_SIZE as u32 + total_body_size) as usize;

        // Reset position
        src.set_position(0);

        Ok(total_len)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bytes::Bytes;

    use crate::memcached::command::SET_OPCODE;
    use crate::memcached::header::{RequestHeader, ResponseHeader};
    use crate::memcached::{MemcachedError, MemcachedErrorKind};

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
        let res = ResponseHeader::check(&mut cursor).expect("Failed to get total length response");
        assert_eq!(res, 29);
    }

    #[test]
    fn check_response_header_bad_magic() {
        let input = "8000000004000000000000050000000000000000000000010000000030";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::check(&mut cursor);
        assert!(res.is_err());
        assert_eq!(
            res.err().unwrap(),
            MemcachedError(MemcachedErrorKind::Other)
        );
    }

    #[test]
    fn check_response_header_incomplete() {
        let input = "81000000040000000000000500000000000000000000";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut cursor = Cursor::new(decoded.as_slice());
        let res = ResponseHeader::check(&mut cursor);
        assert!(res.is_err());
        assert_eq!(
            res.err().unwrap(),
            MemcachedError(MemcachedErrorKind::Incomplete)
        );
    }

    #[test]
    fn header_as_bytes() {
        let input = "800100010800000000000011000000000000000000000000";
        let decoded = hex::decode(input).expect("Decoding failed");
        let key_length: u16 = 1;
        let extra_length = 8;
        let value_length: u32 = 8;
        let mut header = RequestHeader::new(SET_OPCODE, key_length, extra_length, value_length);
        assert_eq!(header.as_bytes(), decoded)
    }
}
