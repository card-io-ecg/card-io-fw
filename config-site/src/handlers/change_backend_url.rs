use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};
use logger::{debug, warn};

use crate::data::SharedWebContext;

pub struct ChangeBackendUrl<'a> {
    pub context: &'a SharedWebContext,
}

impl<C: Connection> RequestHandler<C> for ChangeBackendUrl<'_> {
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

        if !validate_url(post_body) {
            return request
                .send_error_response(ResponseStatus::BadRequest, "Input is not a valid URL")
                .await;
        }

        let result = {
            // Scope-limit the lock guard
            let mut context = self.context.lock().await;
            context.backend_url.clear();
            context.backend_url.push_str(post_body)
        };

        if result.is_err() {
            return request
                .send_error_response(ResponseStatus::BadRequest, "URL is too long")
                .await;
        }

        request.send_response("").await
    }
}

fn validate_url(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }

    const VALID_CHARS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~:/?#[]@!$&'()*+,;=";

    if url.bytes().any(|b| !VALID_CHARS.contains(&b)) {
        return false;
    }

    true
}
