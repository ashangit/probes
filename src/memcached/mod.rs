use std::error::Error;
use std::fmt;
use std::fmt::Display;
use std::io::Cursor;
use std::time::Instant;

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpStream;
use tracing::error;

use crate::memcached::command::{Command, Get, Set};
use crate::memcached::response::Response;
use crate::probes::prometheus::{FAILURE_PROBE, NUMBER_OF_REQUESTS, RESPONSE_TIME_COLLECTOR};

mod command;
mod header;
mod response;

const KEY: &[u8] = "mempoke_key".as_bytes();
const VALUE: &[u8] = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".as_bytes();
const TTL: u64 = 300;

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
    pub fn new(socket: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(socket),
            buffer: BytesMut::with_capacity(1024),
        }
    }

    pub async fn send_request(
        &mut self,
        mut cmd: impl Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.stream.write_all(cmd.as_bytes().as_slice()).await?;
        self.stream.flush().await?;
        Ok(())
    }

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
    pub async fn probe(&mut self) {
        self.set().await;
        self.get().await;
    }

    pub async fn set(&mut self) {
        self.request("set", Set::new(KEY, VALUE, TTL)).await;
    }

    pub async fn get(&mut self) {
        self.request("get", Get::new(KEY)).await;
    }

    pub async fn request(&mut self, cmd_type: &str, cmd: impl Command) {
        let start = Instant::now();

        match self.connection.send_request(cmd).await {
            Ok(_) => {}
            Err(issue) => {
                FAILURE_PROBE
                    .with_label_values(&[self.cluster_name.as_str(), self.addr.as_str()])
                    .inc();
                error!("Failed send request to {}: {}", self.addr, issue);
                return;
            }
        }

        match self.connection.read_response().await {
            Err(issue) => {
                FAILURE_PROBE
                    .with_label_values(&[self.cluster_name.as_str(), self.addr.as_str()])
                    .inc();
                error!("Failed read response from {}: {}", self.addr, issue);
            }
            Ok(mut result) => {
                //debug!("{}", resp.header.status.get_u16());
                NUMBER_OF_REQUESTS
                    .with_label_values(&[
                        self.cluster_name.as_str(),
                        self.addr.as_str(),
                        result.header.status.get_u16().to_string().as_str(),
                        cmd_type,
                    ])
                    .inc();
                // TODO measure only succeed?
                RESPONSE_TIME_COLLECTOR
                    .with_label_values(&[self.cluster_name.as_str(), self.addr.as_str(), cmd_type])
                    .observe(start.elapsed().as_secs_f64());
            }
        }
    }
}
