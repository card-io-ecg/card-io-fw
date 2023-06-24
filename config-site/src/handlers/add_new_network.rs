use core::str::FromStr;

use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::{network::WifiNetwork, SharedWebContext};

pub struct AddNewNetwork<'a> {
    pub context: &'a SharedWebContext,
}

impl<C: Connection> RequestHandler<C> for AddNewNetwork<'_> {
    async fn handle(&self, mut request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let mut buf = [0u8; 100];

        log::debug!("Reading post data");
        let post_data = {
            let mut buffer = &mut buf[..];
            let mut buffered = 0;
            while !request.is_complete() {
                if buffer.is_empty() {
                    log::warn!("POST body too large");
                    let response = request
                        .send_response(ResponseStatus::RequestEntityTooLarge)
                        .await?;
                    return response.start_body().await.map(|_| ());
                }
                log::debug!("Reading...");
                let read = request.read(buffer).await?;
                log::debug!("Read {read} bytes");
                buffer = &mut buffer[read..];
                buffered += read;
            }
            log::debug!("Complete. Read {buffered} bytes");

            &buf[..buffered]
        };

        let post_body = match core::str::from_utf8(post_data) {
            Ok(body) => body,
            Err(err) => {
                log::warn!("Invalid UTF-8 in POST body: {err}");

                let response = request.send_response(ResponseStatus::BadRequest).await?;
                return response.start_body().await.map(|_| ());
            }
        };
        log::debug!("POST body: {post_body:?}");

        let (ssid, pass) = post_body.split_once('\n').unwrap_or((post_body, ""));

        let Ok(ssid) = heapless::String::<32>::from_str(ssid.trim())
        else {
            log::warn!("SSID too long: {ssid}");

            let response = request.send_response(ResponseStatus::BadRequest).await?;
            return response.start_body().await.map(|_| ());
        };

        let Ok(pass) = heapless::String::<64>::from_str(pass.trim())
        else {
            log::warn!("Password too long: {pass}");

            let response = request.send_response(ResponseStatus::BadRequest).await?;
            return response.start_body().await.map(|_| ());
        };

        let result = {
            let mut context = self.context.lock().await;
            context.known_networks.push(WifiNetwork { ssid, pass })
        };

        if result.is_err() {
            log::warn!("Too many networks");

            let response = request.send_response(ResponseStatus::BadRequest).await?;
            return response.start_body().await.map(|_| ());
        }

        let response = request.send_response(ResponseStatus::Ok).await?;
        let mut response = response.start_body().await?;

        response.write_string("0").await?;

        Ok(())
    }
}
