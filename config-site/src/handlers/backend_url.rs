use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::SharedWebContext;

pub struct BackendUrl<'a> {
    pub context: &'a SharedWebContext,
}

impl<C: Connection> RequestHandler<C> for BackendUrl<'_> {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let response = request.start_response(ResponseStatus::Ok).await?;
        let mut response = response.start_chunked_body().await?;

        let context = self.context.lock().await;
        response.write(&context.backend_url).await?;

        response.end_chunked_response().await
    }
}
