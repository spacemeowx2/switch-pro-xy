use bluer::l2cap;
use std::{
    io::Result,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};

struct Inner {
    socket: l2cap::SeqPacket,
    override_id: AtomicBool,
    id: AtomicU8,
    timestmap: AtomicU32,
    start_time: Instant,
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
                override_id: AtomicBool::new(false),
                id: AtomicU8::new(0),
                timestmap: AtomicU32::new(0),
                start_time: Instant::now(),
            }),
        }
    }

    pub fn set_override_id(&self, override_id: bool) {
        self.inner.override_id.store(override_id, Ordering::Relaxed);
    }

    pub async fn send(&self, buf: &[u8]) -> Result<usize> {
        if self.inner.override_id.load(Ordering::Relaxed) && buf.len() >= 4 {
            let mut buf = buf.to_vec();
            let report_id = buf[1];
            if report_id == 0x30 || report_id == 0x21 || report_id == 0x31 {
                let now = self.inner.start_time.elapsed().as_millis() as u32;
                let delta = now - self.inner.timestmap.load(Ordering::Relaxed);
                self.inner.timestmap.store(now, Ordering::Relaxed);
                let elapsed_ticks = delta / 4;
                buf[2] = self
                    .inner
                    .id
                    .fetch_add(elapsed_ticks as u8, Ordering::Relaxed);
            }
            // println!("send: {:x?}", buf);
            return self.inner.socket.send(&buf).await;
        }
        self.inner.socket.send(buf).await
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.inner.socket.recv(buf).await
    }
}
