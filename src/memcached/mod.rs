use std::io::Cursor;

use bytes::{Buf, BytesMut};

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::{TcpStream, ToSocketAddrs};

use crate::memcached::response::Response;
use crate::memcached::set::Set;
use crate::probes::prometheus::NUMBER_OF_REQUESTS;

mod header;
mod response;
mod set;

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

    pub async fn send_request(&mut self, set: &mut Set) {
        self.stream.write_all(set.as_bytes().as_slice()).await;
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
            self.set("nico", "value".as_bytes().to_vec()).await;
        }
    }

    pub async fn set(&mut self, key: &str, value: Vec<u8>) {
        let mut set = Set::new(key, value, 300);
        self.connection.send_request(&mut set).await;
        // TODO manage failure and none
        match self.connection.read_response().await.unwrap() {
            Some(mut resp) => {
                //debug!("{}", resp.header.status.get_u16());
                NUMBER_OF_REQUESTS
                    .with_label_values(&[resp.header.status.get_u16().to_string().as_str(), "set"])
                    .inc()
            }
            None => (),
        }
    }
}
