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

    pub async fn load(
        &mut self,
        count: usize,
        connection: &mut impl Connection,
    ) -> Result<usize, ()> {
        if count <= self.bytes {
            return Ok(self.bytes);
        }

        let end = self.buffer.len().min(count);
        let buffer_to_fill = &mut self.buffer[self.bytes..end];

        let read = connection.read(buffer_to_fill).await.map_err(|_| ())?;
        self.bytes += read;

        Ok(read)
    }

    pub async fn load_exact(
        &mut self,
        count: usize,
        connection: &mut impl Connection,
    ) -> Result<(), ()> {
        while self.bytes < count {
            let read = self.bytes;
            let new_read = self.load(count, connection).await?;
            if new_read == read {
                return Err(());
            }
        }

        Ok(())
    }

    pub async fn read(
        &mut self,
        buf: &mut [u8],
        connection: &mut impl Connection,
    ) -> Result<usize, ()> {
        self.load_exact(buf.len(), connection).await?;

        let buf_len = buf.len();
        let buffer_to_fill = self.flush_loaded(buf);

        Ok(buf_len - buffer_to_fill.len())
    }
}
