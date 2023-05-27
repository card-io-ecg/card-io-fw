use crate::connector::Connection;

pub struct RequestBody<'buf> {
    buffer: &'buf mut [u8],
    bytes: usize,
}

impl<'buf> RequestBody<'buf> {
    pub(crate) fn from_preloaded(buffer: &'buf mut [u8], bytes: usize) -> Self {
        Self { buffer, bytes }
    }

    fn flush_loaded<'r>(&mut self, dst: &'r mut [u8]) -> &'r mut [u8] {
        let loaded = &self.buffer[0..self.bytes];

        let bytes = loaded.len().min(dst.len());
        dst[..bytes].copy_from_slice(&loaded[..bytes]);
        self.bytes -= bytes;

        &mut dst[bytes..]
    }

    pub async fn read(
        &mut self,
        buf: &mut [u8],
        connection: &mut impl Connection,
    ) -> Result<usize, ()> {
        let buf_len = buf.len();
        let buffer_to_fill = self.flush_loaded(buf);
        let read = connection.read(buffer_to_fill).await.map_err(|_| ())?;

        Ok(buf_len - buffer_to_fill.len() + read)
    }
}
