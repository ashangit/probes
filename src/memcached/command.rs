use crate::memcached::header::RequestHeader;

pub const GET_OPCODE: u8 = 0;
pub struct Get {
    header: RequestHeader,
    key: Vec<u8>,
}

pub const SET_OPCODE: u8 = 1;
pub const SET_EXTRA_LEN: u8 = 8;
pub struct Set {
    header: RequestHeader,
    key: Vec<u8>,
    value: Vec<u8>,
    extra_field: Vec<u8>,
}

pub trait Command {
    // Associated function signature; `Self` refers to the implementor type.
    //fn new(name: &'static str) -> Self;

    // Method signatures; these will return a string.
    fn as_bytes(&mut self) -> Vec<u8>;
}

impl Set {
    pub fn new(key: &str, value: &str, expire: u64) -> Set {
        // extra_field from expire
        let extra_field: [u8; SET_EXTRA_LEN as usize] = expire.to_be_bytes();
        let key_vec = key.as_bytes().to_vec();
        let value_vec = value.as_bytes().to_vec();
        let header = RequestHeader::new(
            SET_OPCODE,
            key_vec.len() as u16,
            SET_EXTRA_LEN,
            value_vec.len() as u32,
        );
        Set {
            header,
            key: key_vec,
            value: value_vec,
            extra_field: extra_field.to_vec(),
        }
    }
}

impl Command for Set {
    fn as_bytes(&mut self) -> Vec<u8> {
        let mut req: Vec<u8> = Vec::new();
        req.extend(self.header.as_bytes());
        req.extend(&self.extra_field);
        req.extend(&self.key);
        req.extend(&self.value);
        req
    }
}

impl Get {
    pub fn new(key: &str) -> Get {
        let key_vec = key.as_bytes().to_vec();
        let header = RequestHeader::new(GET_OPCODE, key_vec.len() as u16, 0, 0);
        Get {
            header,
            key: key_vec,
        }
    }
}

impl Command for Get {
    fn as_bytes(&mut self) -> Vec<u8> {
        let mut req: Vec<u8> = Vec::new();
        req.extend(self.header.as_bytes());
        req.extend(&self.key);
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
        let mut set = Set::new("test", "value", 100);
        assert_eq!(set.as_bytes(), decoded)
    }

    #[test]
    fn get_as_bytes() {
        let input = "80000004000000000000000400000000000000000000000074657374";
        let decoded = hex::decode(input).expect("Decoding failed");
        let mut get = Get::new("test");
        assert_eq!(get.as_bytes(), decoded)
    }
}
