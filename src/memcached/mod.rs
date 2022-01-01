use std::io::Cursor;

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, BufWriter};
use tokio::net::{TcpStream, ToSocketAddrs};

use crate::memcached::response::Response;
use crate::memcached::set::Set;

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

    pub async fn send_request(&mut self, _set: &Set) {
        //self.stream.write_all().await?;
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
    pub async fn set(&mut self, key: &str, value: Vec<u8>) {
        let _set = Set::new(key, value, 300);
        //self.connection.send_request(&set).await?;
    }
    //
    // async fn set_cmd(&mut self, cmd: Set) -> crate::Result<()> {
    //     // Convert the `Set` command into a frame
    //     let frame = cmd.into_frame();
    //
    //     debug!(request = ?frame);
    //
    //     // Write the frame to the socket. This writes the full frame to the
    //     // socket, waiting if necessary.
    //     self.connection.write_frame(&frame).await?;
    //
    //     // Wait for the response from the server. On success, the server
    //     // responds simply with `OK`. Any other response indicates an error.
    //     match self.read_response().await? {
    //         Frame::Simple(response) if response == "OK" => Ok(()),
    //         frame => Err(frame.to_error()),
    //     }
    // }
}
