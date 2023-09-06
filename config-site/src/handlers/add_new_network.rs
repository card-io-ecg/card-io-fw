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

        debug!("Reading POST data");
        let post_data = request.read_all(&mut buf).await?;

        if !request.is_complete() {
            return request
                .send_error_response(ResponseStatus::RequestEntityTooLarge, "POST body too large")
                .await;
        }

        let post_body = match core::str::from_utf8(post_data) {
            Ok(body) => body,
            Err(_err) => {
                warn!("Invalid UTF-8 in POST body: {:?}", post_data);
                return request
                    .send_error_response(ResponseStatus::BadRequest, "Input is not valid text")
                    .await;
            }
        };
        debug!("POST body: {:?}", post_body);

        let (ssid, pass) = post_body.split_once('\n').unwrap_or((post_body, ""));

        if ssid.is_empty() {
            return request
                .send_error_response(ResponseStatus::BadRequest, "SSID is empty")
                .await;
        }

        let Ok(ssid) = heapless::String::<32>::from_str(ssid.trim()) else {
            return request
                .send_error_response(ResponseStatus::BadRequest, "SSID too long")
                .await;
        };

        let Ok(pass) = heapless::String::<64>::from_str(pass.trim()) else {
            return request
                .send_error_response(ResponseStatus::BadRequest, "Password too long")
                .await;
        };

        let result = {
            // Scope-limit the lock guard
            let mut context = self.context.lock().await;
            context.known_networks.push(WifiNetwork { ssid, pass })
        };

        if result.is_err() {
            return request
                .send_error_response(ResponseStatus::BadRequest, "Too many networks")
                .await;
        }

        request.send_response("").await
    }
}
