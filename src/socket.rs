use bluer::l2cap;
use futures::task::noop_waker;
use std::{
    io::Result,
    os::unix::prelude::AsRawFd,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use tokio::io::ReadBuf;

struct Inner {
    socket: l2cap::SeqPacket,
    waker: Waker,
}

#[derive(Clone)]
pub struct SeqPacket {
    inner: Arc<Inner>,
}

impl SeqPacket {
    pub fn new(socket: l2cap::SeqPacket) -> Self {
        Self {
            inner: Arc::new(Inner {
                socket,
                waker: noop_waker(),
            }),
        }
    }

    pub async fn send(&self, buf: &[u8]) -> Result<usize> {
        self.inner.socket.send(buf).await
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.inner.socket.recv(buf).await
    }

    pub fn poll_send(&self, buf: &[u8]) -> Poll<Result<usize>> {
        self.inner
            .socket
            .poll_send(&mut Context::from_waker(&self.inner.waker), buf)
    }

    pub fn poll_recv(&self, read_buf: &mut ReadBuf) -> Poll<Result<()>> {
        self.inner
            .socket
            .poll_recv(&mut Context::from_waker(&self.inner.waker), read_buf)
    }

    pub fn get_fd(&self) -> i32 {
        self.inner.socket.as_raw_fd()
    }
}
