use bad_server::{
    connector::Connection, handler::RequestHandler, request::Request, response::ResponseStatus,
    HandleError,
};

use crate::data::SharedWebContext;

pub struct ListKnownNetworks<'a> {
    pub context: &'a SharedWebContext,
}

impl<C: Connection> RequestHandler<C> for ListKnownNetworks<'_> {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let response = request.start_response(ResponseStatus::Ok).await?;
        let mut response = response.start_chunked_body().await?;

        let context = self.context.lock().await;
        for network in context.known_networks.iter() {
            response.write(&network.ssid).await?;
            response.write("\n").await?;
        }

        response.end_chunked_response().await
    }
}
