use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_net::{
    driver::Driver,
    tcp::{
        client::{TcpClient, TcpClientState, TcpConnection},
        Error,
    },
    Stack,
};
use embedded_io::{
    asynch::{Read, Write},
    Io,
};
use embedded_nal_async::{SocketAddr, TcpConnect};
use slice_string::tinyvec::SliceVec;

/// TCP client connection pool compatible with `embedded-nal-async` traits.
///
/// The pool is capable of managing up to N concurrent connections with tx and rx buffers according to TX_SZ and RX_SZ.
pub struct BufferedTcpClient<
    'd,
    D: Driver,
    const N: usize,
    const TX_SZ: usize = 1024,
    const RX_SZ: usize = 1024,
    const W_SZ: usize = 1024,
> {
    inner: TcpClient<'d, D, N, TX_SZ, RX_SZ>,
    pool: &'d Pool<[u8; W_SZ], N>,
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize>
    BufferedTcpClient<'d, D, N, TX_SZ, RX_SZ, W_SZ>
{
    /// Create a new `TcpClient`.
    pub fn new(
        stack: &'d Stack<D>,
        state: &'d BufferedTcpClientState<N, TX_SZ, RX_SZ, W_SZ>,
    ) -> Self {
        Self {
            inner: TcpClient::new(stack, &state.inner),
            pool: &state.pool,
        }
    }
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize>
    TcpConnect for BufferedTcpClient<'d, D, N, TX_SZ, RX_SZ, W_SZ>
{
    type Error = Error;
    type Connection<'m> = BufferedTcpConnection<'m, N, TX_SZ, RX_SZ, W_SZ> where Self: 'm;

    async fn connect<'a>(&'a self, remote: SocketAddr) -> Result<Self::Connection<'a>, Self::Error>
    where
        Self: 'a,
    {
        let connection = self.inner.connect(remote).await?;

        BufferedTcpConnection::new(connection, self.pool)
    }
}

/// Opened TCP connection in a [`BufferedTcpClient`].
pub struct BufferedTcpConnection<
    'd,
    const N: usize,
    const TX_SZ: usize,
    const RX_SZ: usize,
    const W_SZ: usize,
> {
    inner: TcpConnection<'d, N, TX_SZ, RX_SZ>,
    pool: &'d Pool<[u8; W_SZ], N>,
    bufs: NonNull<[u8; W_SZ]>,
    write_buffer: SliceVec<'d, u8>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize>
    BufferedTcpConnection<'d, N, TX_SZ, RX_SZ, W_SZ>
{
    fn new(
        inner: TcpConnection<'d, N, TX_SZ, RX_SZ>,
        pool: &'d Pool<[u8; W_SZ], N>,
    ) -> Result<Self, Error> {
        let bufs = pool.alloc().ok_or(Error::ConnectionReset)?;
        let write_buffer =
            SliceVec::from_slice_len(unsafe { bufs.as_ptr().as_mut().unwrap().as_mut_slice() }, 0);
        Ok(Self {
            inner,
            pool,
            bufs,
            write_buffer,
        })
    }

    async fn write_buffered(&mut self) -> Result<(), Error> {
        if !self.write_buffer.is_empty() {
            self.inner.write(&self.write_buffer).await?;
            self.write_buffer.clear();
        }
        Ok(())
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize> Drop
    for BufferedTcpConnection<'d, N, TX_SZ, RX_SZ, W_SZ>
{
    fn drop(&mut self) {
        unsafe {
            self.pool.free(self.bufs);
        }
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize> Io
    for BufferedTcpConnection<'d, N, TX_SZ, RX_SZ, W_SZ>
{
    type Error = Error;
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize> Read
    for BufferedTcpConnection<'d, N, TX_SZ, RX_SZ, W_SZ>
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.inner.read(buf).await
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize> Write
    for BufferedTcpConnection<'d, N, TX_SZ, RX_SZ, W_SZ>
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.len() > self.write_buffer.capacity() {
            self.write_buffered().await?;
            return self.inner.write(buf).await;
        }

        let space = self.write_buffer.capacity() - self.write_buffer.len();
        let len = buf.len().min(space);

        self.write_buffer.extend_from_slice(&buf[..len]);

        if self.write_buffer.len() == self.write_buffer.capacity() {
            self.write_buffered().await?;
        }

        Ok(len)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.write_buffered().await?;
        self.inner.flush().await
    }
}

/// State for TcpClient
pub struct BufferedTcpClientState<
    const N: usize,
    const TX_SZ: usize,
    const RX_SZ: usize,
    const W_SZ: usize,
> {
    inner: TcpClientState<N, TX_SZ, RX_SZ>,
    pool: Pool<[u8; W_SZ], N>,
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize>
    BufferedTcpClientState<N, TX_SZ, RX_SZ, W_SZ>
{
    /// Create a new `TcpClientState`.
    pub const fn new() -> Self {
        Self {
            inner: TcpClientState::new(),
            pool: Pool::new(),
        }
    }
}

unsafe impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const W_SZ: usize> Sync
    for BufferedTcpClientState<N, TX_SZ, RX_SZ, W_SZ>
{
}

struct Pool<T, const N: usize> {
    used: [AtomicBool; N],
    data: [UnsafeCell<MaybeUninit<T>>; N],
}

impl<T, const N: usize> Pool<T, N> {
    const VALUE: AtomicBool = AtomicBool::new(false);
    const UNINIT: UnsafeCell<MaybeUninit<T>> = UnsafeCell::new(MaybeUninit::uninit());

    const fn new() -> Self {
        Self {
            used: [Self::VALUE; N],
            data: [Self::UNINIT; N],
        }
    }
}

impl<T, const N: usize> Pool<T, N> {
    fn alloc(&self) -> Option<NonNull<T>> {
        for n in 0..N {
            if self.used[n].swap(true, Ordering::SeqCst) == false {
                let p = self.data[n].get() as *mut T;
                return Some(unsafe { NonNull::new_unchecked(p) });
            }
        }
        None
    }

    /// safety: p must be a pointer obtained from self.alloc that hasn't been freed yet.
    unsafe fn free(&self, p: NonNull<T>) {
        let origin = self.data.as_ptr() as *mut T;
        let n = p.as_ptr().offset_from(origin);
        assert!(n >= 0);
        assert!((n as usize) < N);
        self.used[n as usize].store(false, Ordering::SeqCst);
    }
}
