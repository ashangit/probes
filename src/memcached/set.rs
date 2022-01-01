



use crate::memcached::header::RequestHeader;

pub struct Set {
    header: RequestHeader,
    key: Vec<u8>,
    value: Vec<u8>,
    extra_field: Vec<u8>,
}

pub const SET_OPCODE: u8 = 1;
pub const EXTRA_LEN: u8 = 8;

impl Set {
    pub fn new(key: &str, value: Vec<u8>, expire: u64) -> Set {
        // extra_field from expire
        let extra_field: [u8; EXTRA_LEN as usize] = expire.to_be_bytes();
        let key_vec = key.as_bytes().to_vec();
        let header = RequestHeader::new(
            SET_OPCODE,
            key_vec.len() as u16,
            EXTRA_LEN,
            value.len() as u32,
        );
        Set {
            header,
            key: key_vec,
            value,
            extra_field: extra_field.to_vec(),
        }
    }

    pub fn as_bytes(&mut self) -> Vec<u8> {
        let mut request_bytes: Vec<u8> = Vec::new();
        request_bytes.extend(self.header.as_bytes());
        request_bytes.extend(&self.extra_field);
        request_bytes.extend(&self.key);
        request_bytes.extend(&self.value);
        request_bytes
    }
}

#[cfg(test)]
mod tests {
    

    use crate::memcached::set::Set;

    #[test]
    fn set_as_bytes() {
        let input =
            "80010004080000000000001100000000000000000000000000000000000000647465737476616c7565";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut set = Set::new("test", "value".as_bytes().to_vec(), 100);
        assert_eq!(set.as_bytes(), decoded)
    }
}

// REQUEST
// Frame 60: 102 bytes on wire (816 bits), 102 bytes captured (816 bits) on interface any, id 0
// Linux cooked capture v1
// Internet Protocol Version 4, Src: 127.0.0.1, Dst: 127.0.0.1
// Transmission Control Protocol, Src Port: 33252, Dst Port: 11211, Seq: 1, Ack: 1, Len: 34
// Memcache Protocol, Set Request
// Magic: Request (128)
// Opcode: Set (1)
// Key Length: 1
// Extras length: 8
// Data type: Raw bytes (0)
// Reserved: 0
// [Value length: 1]
// Total body length: 10
// Opaque: 0
// CAS: 0
// Extras
// Key: 0
// Value: 0
// 80010001080000000000000a00000000000000000000000000000000000000003030

// RESPONSE
// Frame 62: 92 bytes on wire (736 bits), 92 bytes captured (736 bits) on interface any, id 0
// Linux cooked capture v1
// Internet Protocol Version 4, Src: 127.0.0.1, Dst: 127.0.0.1
// Transmission Control Protocol, Src Port: 11211, Dst Port: 33252, Seq: 1, Ack: 35, Len: 24
// Memcache Protocol, Set Response
// Magic: Response (129)
// Opcode: Set (1)
// Key Length: 0
// Extras length: 0
// Data type: Raw bytes (0)
// Status: No error (0)
// [Value length: 0]
// Total body length: 0
// Opaque: 0
// CAS: 2
// 810100000000000000000000000000000000000000000002
