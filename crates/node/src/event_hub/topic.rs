use crossfire::AsyncTxTrait;
use crossfire::MAsyncTx;
use crossfire::mpsc::Array;
use std::time::Duration;
use tokio::sync::RwLock;

pub(crate) struct Topic<E: 'static> {
    senders: RwLock<Vec<MAsyncTx<Array<E>>>>,
}

impl<E: Send + Sync + Unpin + 'static> Default for Topic<E> {
    fn default() -> Self {
        Self {
            senders: RwLock::new(Vec::new()),
        }
    }
}

impl<E: Send + Sync + Unpin + 'static> Topic<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add_subscriber(&self, tx: MAsyncTx<Array<E>>) {
        let mut write_guard = self.senders.write().await;
        write_guard.retain(|s| !s.is_disconnected());
        write_guard.push(tx);
    }

    pub async fn subscriber_count(&self) -> usize {
        let mut write_guard = self.senders.write().await;
        write_guard.retain(|s| !s.is_disconnected());
        write_guard.len()
    }

    pub async fn publish(&self, event: &E) -> usize
    where
        E: Clone,
    {
        // 1. Clone senders under quick read lock to prevent holding locks during async sends
        let active_senders = {
            let read_guard = self.senders.read().await;
            if read_guard.is_empty() {
                return 0;
            }
            read_guard.clone()
        };

        let mut delivered = 0;
        let mut has_dead = false;

        // 2. Dispatch to subscribers without holding any locks, with bounded timeout to isolate slow subscribers
        for tx in active_senders {
            if tx.is_disconnected() {
                has_dead = true;
                continue;
            }

            match tokio::time::timeout(Duration::from_millis(15), tx.send(event.clone())).await {
                Ok(Ok(())) => {
                    delivered += 1;
                }
                Ok(Err(_)) => {
                    has_dead = true;
                }
                Err(_timeout) => {
                    // Queue full / consumer slow — drop for this specific consumer without stalling others
                }
            }
        }

        // 3. Prune dead subscribers only if disconnected ones were detected
        if has_dead {
            let mut write_guard = self.senders.write().await;
            write_guard.retain(|tx| !tx.is_disconnected());
        }

        delivered
    }
}
