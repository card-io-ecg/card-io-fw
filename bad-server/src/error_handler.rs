use core::{fmt::Write, marker::PhantomData};

use crate::{
    connector::Connection,
    response::{Response, ResponseStatus},
    HandleError,
};

pub trait ErrorHandler {
    type Connection: Connection;

    /// Handles the given error status.
    // TODO: provide a builder instead of the raw connection
    async fn handle(
        &self,
        status: ResponseStatus,
        connection: &mut Self::Connection,
    ) -> Result<(), HandleError<Self::Connection>>;
}

pub struct DefaultErrorHandler<C: Connection>(pub(crate) PhantomData<C>);

impl<C> ErrorHandler for DefaultErrorHandler<C>
where
    C: Connection,
{
    type Connection = C;

    async fn handle(
        &self,
        status: ResponseStatus,
        connection: &mut Self::Connection,
    ) -> Result<(), HandleError<Self::Connection>> {
        ErrorResponse(status)
            .send(connection)
            .await
            .map_err(HandleError::Write)
    }
}

struct ErrorResponse(ResponseStatus);

impl ErrorResponse {
    async fn send<C: Connection>(&self, socket: &mut C) -> Result<(), C::Error> {
        let mut response = Response::new()
            .send_status(self.0, socket)
            .await?
            .end_headers(socket)
            .await?;

        let mut body = heapless::String::<128>::new();
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n",
            code = self.0 as u16,
            reason = self.0.name(),
        );
        response.write_string(&body, socket).await?;

        Ok(())
    }
}
