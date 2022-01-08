use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::Display;
use std::io::Cursor;
use std::time::Instant;

use bytes::{Buf, BytesMut};
use lazy_static::lazy_static;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpStream;

use crate::memcached::command::{Command, Get, Set};
use crate::memcached::response::Response;
use crate::probes::prometheus::{NUMBER_OF_REQUESTS, RESPONSE_TIME_COLLECTOR};

mod command;
mod header;
mod response;

const KEY: &[u8] = "mempoke_key".as_bytes();
const VALUE: &[u8] = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".as_bytes();
const TTL: u64 = 300;

lazy_static! {
    pub static ref STATUS_CODE: HashMap<u16, &'static str> = HashMap::from([
        (0, "NoError"),
        (1, "KeyNotFound"),
        (2, "KeyExists"),
        (3, "ValueTooLarge"),
        (4, "InvalidArguments"),
        (5, "ItemNotStored"),
        (6, "IncrDecrOnNonNumericValue"),
        (129, "UnknownCommand"),
        (130, "OutOfMemory"),
    ]);
}

#[derive(Debug, PartialEq)]
pub struct MemcachedError(MemcachedErrorKind);

#[derive(Debug, Eq, PartialEq)]
pub enum MemcachedErrorKind {
    Incomplete,
    Other,
}

impl MemcachedError {
    fn s(&self) -> &str {
        match self.0 {
            MemcachedErrorKind::Incomplete => "incomplete",
            MemcachedErrorKind::Other => "other issue",
        }
    }
}

impl From<MemcachedErrorKind> for MemcachedError {
    fn from(src: MemcachedErrorKind) -> MemcachedError {
        MemcachedError(src)
    }
}

impl Display for MemcachedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.s().fmt(f)
    }
}

impl Error for MemcachedError {}

pub async fn connect(
    cluster_name: String,
    addr: String,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let socket = TcpStream::connect(addr.clone()).await?;
    let connection = Connection::new(socket);
    Ok(Client {
        cluster_name,
        addr,
        connection,
    })
}

pub struct Connection {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,
}

impl Connection {
    /// Create a new connection to a memcached node
    ///
    /// # Arguments
    ///
    /// * `socket` - tcp stream socket
    ///
    /// # Return
    ///
    /// * Connection
    ///
    pub fn new(socket: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(socket),
            buffer: BytesMut::with_capacity(4096),
        }
    }

    /// Send request to memcached node through the tcp stream
    ///
    /// # Arguments
    ///
    /// * `cmd` - memcached command to execute
    ///
    pub async fn send_request(
        &mut self,
        mut cmd: impl Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.stream.write_all(cmd.as_bytes().as_slice()).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Get response from tcp stream
    ///
    /// Put data from tcp stream in a buffer
    /// The buffer is then parse in parse_response
    /// If not enough data to read the response read again from tcp stream
    ///
    /// # Return
    ///
    /// * Response
    ///
    pub async fn read_response(
        &mut self,
    ) -> Result<Response, Box<dyn std::error::Error + Send + Sync>> {
        loop {
            match self.parse_response() {
                Ok(Some(response)) => return Ok(response),
                Ok(None) => {
                    if 0 == self.stream.read_buf(&mut self.buffer).await? {
                        return if self.buffer.is_empty() {
                            Err("empty or incomplete response".into())
                        } else {
                            Err("connection reset by peer".into())
                        };
                    }
                }
                Err(issue) => return Err(issue.into()),
            };
        }
    }

    /// Parse buffer to get response
    ///
    /// Use a cursor on to of the buffer in order to be able to first check the buffer
    /// and then rewind to really consume the flow of bytes from buffer
    ///
    /// # Return
    ///
    /// * response if there are enough bytes in buffer and no issue face
    ///   or None is there are not enough bytes
    ///   or an Other error from response header check
    ///
    fn parse_response(&mut self) -> Result<Option<Response>, MemcachedError> {
        let mut buf = Cursor::new(&self.buffer[..]);

        match Response::check(&mut buf) {
            Ok(len) => {
                let response = Response::parse(&mut buf);

                self.buffer.advance(len);

                Ok(Some(response))
            }
            Err(MemcachedError(MemcachedErrorKind::Incomplete)) => Ok(None),
            Err(issue) => Err(issue),
        }
    }
}

pub struct Client {
    cluster_name: String,
    addr: String,
    connection: Connection,
}

impl Client {
    /// Probe action
    /// * issue one set
    /// * issue one get
    pub async fn probe(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set().await?;
        self.get().await
    }

    /// Set call
    pub async fn set(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.request("set", Set::new(KEY, VALUE, TTL)).await
    }

    /// Get call
    pub async fn get(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.request("get", Get::new(KEY)).await
    }

    /// Perform memcached request
    ///
    /// # Arguments
    ///
    /// * `cmd_type` - the string represensatation of the command
    /// * `cmd` - the memcached command to perform
    ///
    pub async fn request(
        &mut self,
        cmd_type: &str,
        cmd: impl Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();

        match self.connection.send_request(cmd).await {
            Ok(_) => {}
            Err(issue) => {
                return Err(format!("Failed send request to {}: {}", self.addr, issue).into())
            }
        }

        match self.connection.read_response().await {
            Err(issue) => {
                return Err(format!("Failed read response from {}: {}", self.addr, issue).into());
            }
            Ok(result) => {
                //debug!("{}", resp.header.status.get_u16());
                NUMBER_OF_REQUESTS
                    .with_label_values(&[
                        self.cluster_name.as_str(),
                        self.addr.as_str(),
                        STATUS_CODE.get(&result.header.status).unwrap(),
                        cmd_type,
                    ])
                    .inc();
                // TODO measure only succeed?
                RESPONSE_TIME_COLLECTOR
                    .with_label_values(&[self.cluster_name.as_str(), self.addr.as_str(), cmd_type])
                    .observe(start.elapsed().as_secs_f64());
            }
        }

        Ok(())
    }
}
