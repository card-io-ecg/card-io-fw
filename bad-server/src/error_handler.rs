use core::{fmt::Write as _, marker::PhantomData};

use embedded_io::asynch::Write;

use crate::{
    connector::Connection,
    response::{Response, ResponseStatus},
    HandleError,
};

pub trait ErrorHandler {
    type Connection: Connection;

    /// Handles the given error status.
    async fn handle(
        &self,
        status: ResponseStatus,
        response: Response<'_, Self::Connection>,
    ) -> Result<(), HandleError<Self::Connection>>;
}

pub struct DefaultErrorHandler<C: Write>(pub(crate) PhantomData<C>);

impl<C> ErrorHandler for DefaultErrorHandler<C>
where
    C: Connection,
{
    type Connection = C;

    async fn handle(
        &self,
        status: ResponseStatus,
        response: Response<'_, C>,
    ) -> Result<(), HandleError<C>> {
        let mut response = response.send_status(status).await?.start_body().await?;

        let mut body = heapless::String::<128>::new();
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n",
            code = status as u16,
            reason = status.name(),
        );

        response.write(&body).await
    }
}
