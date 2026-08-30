use crate::BoxError;
use crossfire::AsyncRx;
use crossfire::mpsc::Array;

/// A typed subscriber handle for receiving events of type `E`.
///
/// Backed by a lockless queue from `crossfire`.
pub struct Subscriber<E: 'static> {
    rx: AsyncRx<Array<E>>,
}

impl<E: 'static> Subscriber<E> {
    /// Creates a new `Subscriber` from a crossfire async receiver.
    pub(crate) fn new(rx: AsyncRx<Array<E>>) -> Self {
        Self { rx }
    }

    /// Receives the next event asynchronously.
    ///
    /// Returns `Ok(event)` or an error if the event hub or topic has been closed.
    pub async fn recv(&mut self) -> Result<E, BoxError> {
        let res = self.rx.recv().await;
        res.map_err(|e| {
            Box::new(std::io::Error::new(std::io::ErrorKind::BrokenPipe, e)) as BoxError
        })
    }

    /// Attempts to receive an event without awaiting.
    ///
    /// Returns `Ok(Some(event))` if an event was available, `Ok(None)` if the queue is empty,
    /// or `Err(BoxError)` if disconnected.
    pub fn try_recv(&mut self) -> Result<Option<E>, BoxError> {
        match self.rx.try_recv() {
            Ok(item) => Ok(Some(item)),
            Err(crossfire::TryRecvError::Empty) => Ok(None),
            Err(crossfire::TryRecvError::Disconnected) => Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "disconnected",
            ))),
        }
    }
}
