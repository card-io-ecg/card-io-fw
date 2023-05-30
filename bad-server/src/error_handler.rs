use core::{fmt::Write, marker::PhantomData};

use httparse::Header;

use crate::{
    connector::Connection,
    response::{Body, Headers, Initial, Response, ResponseState, ResponseStatus},
    HandleError,
};

pub trait ErrorHandler {
    type Connection: Connection;

    /// Handles the given error status.
    async fn handle(
        &self,
        status: ResponseStatus,
        response: ResponseBuilder<'_, Self::Connection>,
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
        response: ResponseBuilder<'_, Self::Connection>,
    ) -> Result<(), HandleError<Self::Connection>> {
        let mut response = response.send_status(status).await?.end_headers().await?;

        let mut body = heapless::String::<128>::new();
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n",
            code = status as u16,
            reason = status.name(),
        );

        response.write_string(&body).await
    }
}

/// Response builder for the error handler
pub struct ResponseBuilder<'resp, C, S = Initial>
where
    S: ResponseState,
    C: Connection,
{
    socket: &'resp mut C,
    response: Response<S>,
}

impl<'resp, C> ResponseBuilder<'resp, C, Initial>
where
    C: Connection,
{
    pub fn new(socket: &'resp mut C) -> Self {
        Self {
            socket,
            response: Response::new(),
        }
    }

    pub async fn send_status(
        self,
        status: ResponseStatus,
    ) -> Result<ResponseBuilder<'resp, C, Headers>, HandleError<C>> {
        Ok(ResponseBuilder {
            response: self
                .response
                .send_status(status, self.socket)
                .await
                .map_err(HandleError::Write)?,
            socket: self.socket,
        })
    }
}

impl<'resp, C> ResponseBuilder<'resp, C, Headers>
where
    C: Connection,
{
    pub async fn send_header(&mut self, header: Header<'_>) -> Result<&mut Self, HandleError<C>> {
        self.response
            .send_header(header, self.socket)
            .await
            .map_err(HandleError::Write)?;
        Ok(self)
    }

    pub async fn send_headers(
        &mut self,
        headers: &[Header<'_>],
    ) -> Result<&mut Self, HandleError<C>> {
        self.response
            .send_headers(headers, self.socket)
            .await
            .map_err(HandleError::Write)?;
        Ok(self)
    }

    pub async fn end_headers(self) -> Result<ResponseBuilder<'resp, C, Body>, HandleError<C>> {
        Ok(ResponseBuilder {
            response: self
                .response
                .end_headers(self.socket)
                .await
                .map_err(HandleError::Write)?,
            socket: self.socket,
        })
    }
}

impl<'resp, C> ResponseBuilder<'resp, C, Body>
where
    C: Connection,
{
    pub async fn write_string(&mut self, data: &str) -> Result<(), HandleError<C>> {
        self.response
            .write_string(data, self.socket)
            .await
            .map_err(HandleError::Write)
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), HandleError<C>> {
        self.response
            .write_raw(data, self.socket)
            .await
            .map_err(HandleError::Write)
    }
}
