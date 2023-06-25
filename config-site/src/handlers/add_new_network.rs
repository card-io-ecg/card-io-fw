use core::str::FromStr;

use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::{network::WifiNetwork, SharedWebContext};

pub struct AddNewNetwork<'a> {
    pub context: &'a SharedWebContext,
}

impl<'a> AddNewNetwork<'a> {
    async fn request_error<C: Connection>(
        &self,
        request: Request<'_, '_, C>,
        status: ResponseStatus,
    ) -> Result<(), HandleError<C>> {
        request
            .send_response(status)
            .await?
            .start_body()
            .await
            .map(|_| ())
    }
}

impl<C: Connection> RequestHandler<C> for AddNewNetwork<'_> {
    async fn handle(&self, mut request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let mut buf = [0u8; 100];

        log::debug!("Reading POST data");
        let post_data = request.read_all(&mut buf).await?;

        if !request.is_complete() {
            log::warn!("POST body too large");
            return self
                .request_error(request, ResponseStatus::RequestEntityTooLarge)
                .await;
        }

        let post_body = match core::str::from_utf8(post_data) {
            Ok(body) => body,
            Err(err) => {
                log::warn!("Invalid UTF-8 in POST body: {err}");
                return self
                    .request_error(request, ResponseStatus::BadRequest)
                    .await;
            }
        };
        log::debug!("POST body: {post_body:?}");

        let (ssid, pass) = post_body.split_once('\n').unwrap_or((post_body, ""));

        let Ok(ssid) = heapless::String::<32>::from_str(ssid.trim())
        else {
            log::warn!("SSID too long: {ssid}");
            return self.request_error(request,ResponseStatus::BadRequest).await;
        };

        let Ok(pass) = heapless::String::<64>::from_str(pass.trim())
        else {
            log::warn!("Password too long: {pass}");
            return self.request_error(request,ResponseStatus::BadRequest).await;
        };

        let result = {
            let mut context = self.context.lock().await;
            context.known_networks.push(WifiNetwork { ssid, pass })
        };

        if result.is_err() {
            log::warn!("Too many networks");
            return self
                .request_error(request, ResponseStatus::BadRequest)
                .await;
        }

        let response = request.send_response(ResponseStatus::Ok).await?;
        response.start_body().await.map(|_| ())
    }
}
