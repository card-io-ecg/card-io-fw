use core::str::FromStr;

use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::SharedWebContext;

pub struct DeleteNetwork<'a> {
    pub context: &'a SharedWebContext,
}

impl<'a> DeleteNetwork<'a> {
    async fn request_error<C: Connection>(
        &self,
        request: Request<'_, '_, C>,
        status: ResponseStatus,
        message: &str,
    ) -> Result<(), HandleError<C>> {
        log::warn!("Request error: {:?}, {}", status, message);
        request
            .send_response(status)
            .await?
            .start_body()
            .await?
            .write_string(message)
            .await
    }
}

impl<C: Connection> RequestHandler<C> for DeleteNetwork<'_> {
    async fn handle(&self, mut request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let mut buf = [0u8; 100];

        log::debug!("Reading POST data");
        let post_data = request.read_all(&mut buf).await?;

        if !request.is_complete() {
            return self
                .request_error(
                    request,
                    ResponseStatus::RequestEntityTooLarge,
                    "POST body too large",
                )
                .await;
        }

        let post_body = match core::str::from_utf8(post_data) {
            Ok(body) => body,
            Err(err) => {
                log::warn!("Invalid UTF-8 in POST body: {}", err);
                return self
                    .request_error(
                        request,
                        ResponseStatus::BadRequest,
                        "Input is not valid text",
                    )
                    .await;
            }
        };
        log::debug!("POST body: {:?}", post_body);

        let index = match usize::from_str(post_body) {
            Ok(index) => index,
            Err(err) => {
                log::warn!("Invalid index in POST body: {}", err);
                return self
                    .request_error(
                        request,
                        ResponseStatus::BadRequest,
                        "Network index is not a valid number",
                    )
                    .await;
            }
        };

        {
            // Scope-limit the lock guard
            let mut context = self.context.lock().await;
            if index < context.known_networks.len() {
                context.known_networks.swap_remove(index);
            }
        };

        let response = request.send_response(ResponseStatus::Ok).await?;
        response.start_body().await.map(|_| ())
    }
}
