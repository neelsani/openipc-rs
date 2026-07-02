use std::collections::VecDeque;

/// Behavior when a bounded queue reaches capacity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DropPolicy {
    /// Reject the incoming item.
    DropNewest,
    /// Remove the oldest queued item and keep the incoming item.
    #[default]
    KeepLatest,
}

/// Result of inserting into a bounded queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePush {
    /// Item was added without dropping another item.
    Added,
    /// Oldest item was replaced.
    ReplacedOldest,
    /// Incoming item was rejected.
    DroppedNewest,
}

/// Small bounded FIFO for latency-sensitive media work.
#[derive(Debug)]
pub struct BoundedQueue<T> {
    items: VecDeque<T>,
    capacity: usize,
    policy: DropPolicy,
}

impl<T> BoundedQueue<T> {
    /// Create a queue with a non-zero capacity.
    pub fn new(capacity: usize, policy: DropPolicy) -> Option<Self> {
        (capacity > 0).then(|| Self {
            items: VecDeque::with_capacity(capacity),
            capacity,
            policy,
        })
    }

    /// Push an item according to the configured drop policy.
    pub fn push(&mut self, item: T) -> QueuePush {
        if self.items.len() < self.capacity {
            self.items.push_back(item);
            return QueuePush::Added;
        }
        match self.policy {
            DropPolicy::DropNewest => QueuePush::DroppedNewest,
            DropPolicy::KeepLatest => {
                self.items.pop_front();
                self.items.push_back(item);
                QueuePush::ReplacedOldest
            }
        }
    }

    /// Pop the oldest queued item.
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// Number of queued items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no items are queued.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Remove every queued item.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedQueue, DropPolicy, QueuePush};

    #[test]
    fn keep_latest_discards_oldest_item() {
        let mut queue = BoundedQueue::new(2, DropPolicy::KeepLatest).unwrap();
        queue.push(1);
        queue.push(2);
        assert_eq!(queue.push(3), QueuePush::ReplacedOldest);
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }

    #[test]
    fn drop_newest_preserves_queued_items() {
        let mut queue = BoundedQueue::new(1, DropPolicy::DropNewest).unwrap();
        queue.push(1);
        assert_eq!(queue.push(2), QueuePush::DroppedNewest);
        assert_eq!(queue.pop(), Some(1));
    }
}
