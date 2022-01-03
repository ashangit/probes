use std::io::Cursor;
use std::time::Instant;

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::{TcpStream, ToSocketAddrs};

use crate::memcached::command::{Command, Get, Set};
use crate::memcached::response::Response;
use crate::probes::prometheus::{NUMBER_OF_REQUESTS, RESPONSE_TIME_COLLECTOR};

mod command;
mod header;
mod response;

pub async fn connect<T: ToSocketAddrs>(
    addr: T,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let socket = TcpStream::connect(addr).await?;
    let connection = Connection::new(socket);

    Ok(Client { connection })
}

pub struct Connection {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,
}

impl Connection {
    pub fn new(socket: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(socket),
            buffer: BytesMut::with_capacity(1 * 1024),
        }
    }

    pub async fn send_request(&mut self, mut cmd: impl Command) {
        self.stream.write_all(cmd.as_bytes().as_slice()).await;
        self.stream.flush().await;
    }

    pub async fn read_response(
        &mut self,
    ) -> Result<Option<Response>, Box<dyn std::error::Error + Send + Sync>> {
        loop {
            if let Some(response) = self.parse_response()? {
                return Ok(Some(response));
            }

            if 0 == self.stream.read_buf(&mut self.buffer).await? {
                if self.buffer.is_empty() {
                    return Ok(None);
                } else {
                    return Err("connection reset by peer".into());
                }
            }
        }
    }

    fn parse_response(
        &mut self,
    ) -> Result<Option<Response>, Box<dyn std::error::Error + Send + Sync>> {
        use response::Error::Incomplete;
        let mut buf = Cursor::new(&self.buffer[..]);

        match Response::check(&mut buf) {
            Ok(len) => {
                let response = Response::parse(&mut buf);

                self.buffer.advance(len);

                Ok(Some(response))
            }
            Err(Incomplete) => Ok(None),
            Err(_e) => Err("Failure parsing rsponse".into()),
        }
    }
}

pub struct Client {
    connection: Connection,
}

impl Client {
    pub async fn probe(&mut self) {
        loop {
            self.set("nico", "value").await;
            self.get("nico").await;
        }
    }

    pub async fn set(&mut self, key: &str, value: &str) {
        let set = Set::new(key, value, 300);
        self.request("set", set).await;
    }

    pub async fn get(&mut self, key: &str) {
        let get = Get::new(key);
        self.request("get", get).await;
    }

    pub async fn request(&mut self, cmd_type: &str, cmd: impl Command) {
        let start = Instant::now();
        self.connection.send_request(cmd).await;
        // TODO manage failure and none
        match self.connection.read_response().await.unwrap() {
            Some(mut resp) => {
                //debug!("{}", resp.header.status.get_u16());
                NUMBER_OF_REQUESTS
                    .with_label_values(&[
                        resp.header.status.get_u16().to_string().as_str(),
                        cmd_type,
                    ])
                    .inc();
                // TODO measure only succeed?
                RESPONSE_TIME_COLLECTOR
                    .with_label_values(&[cmd_type])
                    .observe(start.elapsed().as_secs_f64());
            }
            None => (),
        }
    }
}
