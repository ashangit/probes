use std::time::Duration;

use bytes::Bytes;

pub struct Set {
    /// the lookup key
    key: String,

    /// the value to be stored
    value: Bytes,

    /// When to expire the key
    expire: Option<Duration>,
}

impl Set {
    /// Create a new `Set` command which sets `key` to `value`.
    ///
    /// If `expire` is `Some`, the value should expire after the specified
    /// duration.
    pub fn new(key: impl ToString, value: Bytes, expire: Option<Duration>) -> Set {
        Set {
            key: key.to_string(),
            value,
            expire,
        }
    }

    // /// Converts the command into an equivalent `Frame`.
    // ///
    // /// This is called by the client when encoding a `Set` command to send to
    // /// the server.
    // pub(crate) fn into_frame(self) -> Frame {}
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
