use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::io::Cursor;
use std::time::{Duration, Instant};

use bytes::{Buf, BytesMut};
use lazy_static::lazy_static;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpStream;
use tokio::time::error::Elapsed;

use crate::memcached::command::{Command, Get, Set};
use crate::memcached::response::Response;
use crate::probes::prometheus::{NUMBER_OF_REQUESTS, RESPONSE_TIME_COLLECTOR};

mod command;
mod header;
mod response;

const KEY: &[u8] = "mempoke_key".as_bytes();
const VALUE: &[u8] = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".as_bytes();
const TTL: u64 = 300;

const TIMEOUT: Duration = Duration::from_millis(100);

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
pub enum MemcachedError {
    Incomplete,
    Other,
}

impl Display for MemcachedError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            MemcachedError::Incomplete => write!(f, "incomplete."),
            MemcachedError::Other => write!(f, "other issue."),
        }
    }
}

impl Error for MemcachedError {}

#[derive(Debug)]
pub enum MemcachedClientError {
    EmptyOrIncompleteResponse,
    Io(io::Error),
    ConnectionReset,
    MemcachedError(MemcachedError),
    Timeout(Elapsed),
}

impl Display for MemcachedClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            MemcachedClientError::EmptyOrIncompleteResponse => {
                write!(f, "empty or incomplete response.")
            }
            MemcachedClientError::Io(ref io) => write!(f, "I/O error: {}", io),
            MemcachedClientError::Timeout(ref timeout) => write!(f, "Timeout error: {}", timeout),
            MemcachedClientError::ConnectionReset => write!(f, "Connection reset by peer."),
            MemcachedClientError::MemcachedError(ref memcached_error) => {
                write!(f, "MemcachedError error: {}", memcached_error)
            }
        }
    }
}

impl From<io::Error> for MemcachedClientError {
    fn from(other: io::Error) -> Self {
        MemcachedClientError::Io(other)
    }
}

impl From<MemcachedError> for MemcachedClientError {
    fn from(other: MemcachedError) -> Self {
        MemcachedClientError::MemcachedError(other)
    }
}

impl From<Elapsed> for MemcachedClientError {
    fn from(other: Elapsed) -> Self {
        MemcachedClientError::Timeout(other)
    }
}

impl Error for MemcachedClientError {}

pub async fn connect(cluster_name: &str, addr: &str) -> Result<Client, MemcachedClientError> {
    let socket = TcpStream::connect(addr).await?;
    let connection = Connection::new(socket);
    Ok(Client {
        cluster_name: cluster_name.to_owned(),
        addr: addr.to_owned(),
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
    pub fn new(socket: TcpStream) -> Self {
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
    ) -> Result<(), MemcachedClientError> {
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
    pub async fn read_response(&mut self) -> Result<Response, MemcachedClientError> {
        loop {
            match self.parse_response() {
                Ok(Some(response)) => return Ok(response),
                Ok(None) => {
                    if 0 == self.stream.read_buf(&mut self.buffer).await? {
                        return if self.buffer.is_empty() {
                            Err(MemcachedClientError::EmptyOrIncompleteResponse)
                        } else {
                            Err(MemcachedClientError::ConnectionReset)
                        };
                    }
                }
                Err(issue) => return Err(issue),
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
    fn parse_response(&mut self) -> Result<Option<Response>, MemcachedClientError> {
        let mut buf = Cursor::new(&self.buffer[..]);

        match Response::check(&mut buf) {
            Ok(len) => {
                let response = Response::parse(&mut buf);

                self.buffer.advance(len);

                Ok(Some(response))
            }
            Err(MemcachedError::Incomplete) => Ok(None),
            Err(issue) => Err(MemcachedClientError::from(issue)),
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
    pub async fn probe(&mut self) -> Result<(), MemcachedClientError> {
        self.set().await?;
        self.get().await
    }

    /// Set call
    pub async fn set(&mut self) -> Result<(), MemcachedClientError> {
        self.handler_with_timeout("set", Set::new(KEY, VALUE, TTL))
            .await
    }

    /// Get call
    pub async fn get(&mut self) -> Result<(), MemcachedClientError> {
        self.handler_with_timeout("get", Get::new(KEY)).await
    }

    async fn handler_with_timeout(
        &mut self,
        cmd_type: &str,
        cmd: impl Command,
    ) -> Result<(), MemcachedClientError> {
        match tokio::time::timeout(TIMEOUT, self.handle_request(cmd_type, cmd)).await {
            Ok(Err(error)) => Err(error),
            Err(_timeout_elapsed) => {
                RESPONSE_TIME_COLLECTOR
                    .with_label_values(&[self.cluster_name.as_str(), self.addr.as_str(), cmd_type])
                    .observe(TIMEOUT.as_secs_f64());
                Err(MemcachedClientError::from(_timeout_elapsed))
            }
            _ => Ok(()),
        }
    }

    /// Perform memcached request
    ///
    /// # Arguments
    ///
    /// * `cmd_type` - the string represensatation of the command
    /// * `cmd` - the memcached command to perform
    ///
    pub async fn handle_request(
        &mut self,
        cmd_type: &str,
        cmd: impl Command,
    ) -> Result<(), MemcachedClientError> {
        let start = Instant::now();

        self.connection.send_request(cmd).await?;

        match self.connection.read_response().await {
            Err(issue) => Err(issue),
            Ok(result) => {
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
                Ok(())
            }
        }
    }
}
