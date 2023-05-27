pub struct RequestBody<'buf> {
    buffer: &'buf mut [u8],
    bytes: usize,
}

impl<'buf> RequestBody<'buf> {
    pub(crate) fn from_preloaded(buffer: &'buf mut [u8], bytes: usize) -> Self {
        Self { buffer, bytes }
    }
}
