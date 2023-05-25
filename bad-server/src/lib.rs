#![no_std]

use embassy_net::{driver::Driver, tcp::TcpSocket, IpListenEndpoint, Stack};
use embedded_io::asynch::Write;

pub struct BadServer<'s, D: Driver> {
    stack: &'s Stack<D>,
    rx_buffer: &'s mut [u8],
    tx_buffer: &'s mut [u8],
}

impl<'s, D: Driver> BadServer<'s, D> {
    pub fn build(stack: &'s Stack<D>, rx_buffer: &'s mut [u8], tx_buffer: &'s mut [u8]) -> Self {
        Self {
            stack,
            rx_buffer,
            tx_buffer,
        }
    }

    pub async fn listen(self, port: u16) {
        let mut socket = TcpSocket::new(self.stack, self.rx_buffer, self.tx_buffer);
        socket.set_timeout(Some(embassy_net::SmolDuration::from_secs(10)));

        loop {
            log::info!("Wait for connection...");

            let r = socket.accept(IpListenEndpoint { addr: None, port }).await;

            log::info!("Connected...");

            if let Err(e) = r {
                log::warn!("connect error: {:?}", e);
                continue;
            }

            let mut buffer = [0u8; 1024];
            let mut pos = 0;

            loop {
                let len = match socket.read(&mut buffer).await {
                    Ok(0) => {
                        log::info!("read EOF");
                        break;
                    }
                    Ok(len) => len,
                    Err(e) => {
                        log::warn!("read error: {:?}", e);
                        break;
                    }
                };

                pos += len;
                log::info!("Buffer size: {pos}");

                let mut headers = [httparse::EMPTY_HEADER; 20];
                let mut req = httparse::Request::new(&mut headers);

                let res = match req.parse(&buffer) {
                    Ok(res) => res,
                    Err(_) => {
                        log::warn!("Parsing request failed");
                        socket.close();
                        continue;
                    }
                };
                if res.is_complete() {
                    let r = socket
                        .write_all(
                            b"HTTP/1.0 200 OK\r\n\r\n\
                            <html>\
                                <body>\
                                    <h1>Hello Rust! Hello esp-wifi!</h1>\
                                </body>\
                            </html>\r\n\
                            ",
                        )
                        .await;

                    if let Err(e) = r {
                        log::warn!("write error: {:?}", e);
                    }

                    if let Err(e) = socket.flush().await {
                        log::warn!("flush error: {:?}", e);
                    }

                    pos = 0;
                }
            }
        }
    }
}
