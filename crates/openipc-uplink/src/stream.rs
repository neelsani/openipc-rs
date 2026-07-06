use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(crate) const STREAM_QUEUE_CAPACITY: usize = 256 * 1024;

#[derive(Debug, Default)]
pub(crate) struct StreamState {
    pub(crate) received: VecDeque<u8>,
    pub(crate) pending_send: VecDeque<u8>,
    pub(crate) connected: bool,
    pub(crate) remote_closed: bool,
    pub(crate) local_closed: bool,
    pub(crate) error: Option<String>,
    pub(crate) read_waker: Option<Waker>,
    pub(crate) write_waker: Option<Waker>,
    pub(crate) flush_waker: Option<Waker>,
}

impl StreamState {
    pub(crate) fn wake_all(&mut self) {
        if let Some(waker) = self.read_waker.take() {
            waker.wake();
        }
        if let Some(waker) = self.write_waker.take() {
            waker.wake();
        }
        if let Some(waker) = self.flush_waker.take() {
            waker.wake();
        }
    }
}

/// A TCP byte stream backed by the userspace OpenIPC tunnel network.
///
/// This implements Tokio's I/O traits without depending on a Tokio reactor or
/// an operating-system socket. It is therefore usable by the same SSH client on
/// native targets and in browser WebAssembly.
#[derive(Debug, Clone)]
pub struct VirtualTcpStream {
    pub(crate) state: Arc<Mutex<StreamState>>,
}

impl VirtualTcpStream {
    pub(crate) fn new(state: Arc<Mutex<StreamState>>) -> Self {
        Self { state }
    }

    /// Returns whether the TCP handshake has completed.
    pub fn is_connected(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.connected)
    }
}

impl AsyncRead for VirtualTcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("virtual TCP stream state poisoned"))?;
        if let Some(error) = state.error.as_ref() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                error.clone(),
            )));
        }
        let amount = output.remaining().min(state.received.len());
        if amount != 0 {
            let (first, second) = state.received.as_slices();
            let first_amount = amount.min(first.len());
            output.put_slice(&first[..first_amount]);
            if first_amount < amount {
                output.put_slice(&second[..amount - first_amount]);
            }
            state.received.drain(..amount);
            return Poll::Ready(Ok(()));
        }
        if state.remote_closed {
            return Poll::Ready(Ok(()));
        }
        state.read_waker = Some(context.waker().clone());
        Poll::Pending
    }
}

impl AsyncWrite for VirtualTcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("virtual TCP stream state poisoned"))?;
        if state.local_closed || state.remote_closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "virtual TCP stream is closed",
            )));
        }
        if let Some(error) = state.error.as_ref() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                error.clone(),
            )));
        }
        let available = STREAM_QUEUE_CAPACITY.saturating_sub(state.pending_send.len());
        let amount = available.min(input.len());
        if amount == 0 {
            state.write_waker = Some(context.waker().clone());
            return Poll::Pending;
        }
        state.pending_send.extend(&input[..amount]);
        Poll::Ready(Ok(amount))
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("virtual TCP stream state poisoned"))?;
        if state.pending_send.is_empty() {
            return Poll::Ready(Ok(()));
        }
        state.flush_waker = Some(context.waker().clone());
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("virtual TCP stream state poisoned"))?;
        state.local_closed = true;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, sync::Arc, task::Poll};

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use super::{StreamState, VirtualTcpStream};

    #[test]
    fn stream_moves_bytes_without_platform_socket() {
        let state = Arc::new(std::sync::Mutex::new(StreamState::default()));
        let mut stream = VirtualTcpStream::new(Arc::clone(&state));
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);

        assert!(matches!(
            Pin::new(&mut stream).poll_write(&mut context, b"request"),
            Poll::Ready(Ok(7))
        ));
        assert_eq!(
            state
                .lock()
                .unwrap()
                .pending_send
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            b"request"
        );

        state.lock().unwrap().received.extend(b"response");
        let mut bytes = [0; 8];
        let mut output = ReadBuf::new(&mut bytes);
        assert!(matches!(
            Pin::new(&mut stream).poll_read(&mut context, &mut output),
            Poll::Ready(Ok(()))
        ));
        assert_eq!(output.filled(), b"response");
    }
}
