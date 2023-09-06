use core::str::FromStr;

use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::SharedWebContext;

pub struct DeleteNetwork<'a> {
    pub context: &'a SharedWebContext,
}

impl<C: Connection> RequestHandler<C> for DeleteNetwork<'_> {
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

        let index = match usize::from_str(post_body) {
            Ok(index) => index,
            Err(_err) => {
                warn!("Invalid index in POST body: {:?}", post_body);
                return request
                    .send_error_response(
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

        request.send_response("").await
    }
}
