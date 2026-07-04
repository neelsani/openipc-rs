use std::collections::VecDeque;

/// Bounded queue that only resumes after overload at a fresh keyframe.
pub(crate) struct LowLatencyQueue<T> {
    items: VecDeque<T>,
    capacity: usize,
    waiting_for_keyframe: bool,
    dropped: u64,
}

impl<T> LowLatencyQueue<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            items: VecDeque::with_capacity(capacity),
            capacity,
            waiting_for_keyframe: false,
            dropped: 0,
        }
    }

    pub(crate) fn push(&mut self, item: T, is_keyframe: bool) {
        if self.items.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(self.items.len() as u64);
            self.items.clear();
            self.waiting_for_keyframe = true;
        }

        if self.waiting_for_keyframe {
            if !is_keyframe {
                self.dropped = self.dropped.saturating_add(1);
                return;
            }
            self.waiting_for_keyframe = false;
        }
        self.items.push_back(item);
    }

    pub(crate) fn force_resync(&mut self) {
        self.dropped = self.dropped.saturating_add(self.items.len() as u64);
        self.items.clear();
        self.waiting_for_keyframe = true;
    }

    pub(crate) fn pop_front(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::LowLatencyQueue;

    #[test]
    fn preserves_frames_below_capacity() {
        let mut queue = LowLatencyQueue::new(3);
        queue.push(1, true);
        queue.push(2, false);
        queue.push(3, false);
        assert_eq!(queue.len(), 3);
        assert!(!queue.is_empty());
        assert_eq!(queue.pop_front(), Some(1));
        assert_eq!(queue.pop_front(), Some(2));
        assert_eq!(queue.pop_front(), Some(3));
        assert!(queue.is_empty());
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn discards_overload_until_a_keyframe() {
        let mut queue = LowLatencyQueue::new(2);
        queue.push(1, true);
        queue.push(2, false);
        queue.push(3, false);
        queue.push(4, false);
        queue.push(5, true);
        queue.push(6, false);

        assert_eq!(queue.pop_front(), Some(5));
        assert_eq!(queue.pop_front(), Some(6));
        assert_eq!(queue.dropped(), 4);
    }

    #[test]
    fn forced_resync_drops_queued_delta_frames() {
        let mut queue = LowLatencyQueue::new(3);
        queue.push(1, true);
        queue.push(2, false);
        queue.force_resync();
        queue.push(3, false);
        queue.push(4, true);

        assert_eq!(queue.pop_front(), Some(4));
        assert_eq!(queue.dropped(), 3);
    }
}
