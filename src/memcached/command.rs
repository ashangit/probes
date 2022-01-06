use crate::memcached::header::RequestHeader;

const SET_EXTRA_LEN: u8 = 8;

pub const GET_OPCODE: u8 = 0;

pub struct Get {
    header: RequestHeader,
    key: &'static [u8],
}

pub const SET_OPCODE: u8 = 1;

pub struct Set {
    header: RequestHeader,
    key: &'static [u8],
    value: &'static [u8],
    extra_field: [u8; SET_EXTRA_LEN as usize],
}

impl Set {
    /// Create a new Set command
    ///
    /// # Arguments
    ///
    /// * `key` - the key as bytes
    /// * `value` - the value as bytes
    /// * `ttl` - the ttl of the item
    ///
    /// # Return
    ///
    /// * Set
    ///
    pub fn new(key: &'static [u8], value: &'static [u8], ttl: u64) -> Set {
        let extra_field: [u8; SET_EXTRA_LEN as usize] = ttl.to_be_bytes();

        let header = RequestHeader::new(
            SET_OPCODE,
            key.len() as u16,
            SET_EXTRA_LEN,
            value.len() as u32,
        );
        Set {
            header,
            key,
            value,
            extra_field,
        }
    }
}

impl Get {
    /// Create a new Get command
    ///
    /// # Arguments
    ///
    /// * `key` - the key as bytes
    ///
    /// # Return
    ///
    /// * Get
    ///
    pub fn new(key: &'static [u8]) -> Get {
        let header = RequestHeader::new(GET_OPCODE, key.len() as u16, 0, 0);
        Get { header, key }
    }
}

pub trait Command {
    fn as_bytes(&mut self) -> Vec<u8>;
}

impl Command for Set {
    /// Return representation of Set as bytes
    fn as_bytes(&mut self) -> Vec<u8> {
        let mut req: Vec<u8> = Vec::new();
        req.extend(self.header.as_bytes());
        req.extend(&self.extra_field);
        req.extend(self.key);
        req.extend(self.value);
        req
    }
}

impl Command for Get {
    /// Return representation of Get as bytes
    fn as_bytes(&mut self) -> Vec<u8> {
        let mut req: Vec<u8> = Vec::new();
        req.extend(self.header.as_bytes());
        req.extend(self.key);
        req
    }
}

#[cfg(test)]
mod tests {
    use crate::memcached::command::{Command, Get, Set};

    #[test]
    fn set_as_bytes() {
        let input =
            "80010004080000000000001100000000000000000000000000000000000000647465737476616c7565";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut set = Set::new("test".as_bytes(), "value".as_bytes(), 100);
        assert_eq!(set.as_bytes(), decoded)
    }

    #[test]
    fn get_as_bytes() {
        let input = "80000004000000000000000400000000000000000000000074657374";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut get = Get::new("test".as_bytes());
        assert_eq!(get.as_bytes(), decoded)
    }
}
