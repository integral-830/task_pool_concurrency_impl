use std::sync::{Arc, RwLock};

use crate::storage::buffer_pool::manager::BufferPoolError;

use super::frame::Frame;

pub type FrameId = usize;

pub struct ClockReplacer {
    clock_hand: usize,
}

impl ClockReplacer {
    pub fn new() -> Self {
        Self { clock_hand: 0 }
    }

    pub fn find_victim(
        &mut self,
        frames: &[Arc<RwLock<Frame>>],
    ) -> Result<Option<FrameId>, BufferPoolError> {
        let len = frames.len();

        if len == 0 {
            return Ok(None);
        }

        loop {
            let mut scanned = 0;
            let mut found_unpinned = false;

            while scanned < len {
                let id = self.clock_hand;

                let mut frame = frames[id]
                    .write()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                if frame.is_pinned() {
                    self.advance_hand(len);
                } else {
                    if frame.is_reserved() {
                        self.advance_hand(len);
                    } else {
                        found_unpinned = true;

                        if frame.is_referenced() {
                            frame
                                .mark_unreferenced()
                                .map_err(BufferPoolError::FrameError)?;

                            self.advance_hand(len);
                        } else {
                            self.advance_hand(len);
                            return Ok(Some(id));
                        }
                    }
                }

                scanned += 1;
            }

            // If no unpinned frame was found during the
            // entire revolution, no frame is evictable.
            if !found_unpinned {
                return Ok(None);
            }

            // At least one unpinned frame existed, but all
            // such frames had their reference bit set.
            // They now have a second chance, so scan again.
        }
    }

    fn advance_hand(&mut self, len: usize) {
        debug_assert!(len > 0);
        debug_assert!(self.clock_hand < len);
        self.clock_hand = (self.clock_hand + 1) % len;
    }

    pub fn clock_hand(&self) -> usize {
        self.clock_hand
    }
}

impl Default for ClockReplacer {
    fn default() -> Self {
        Self::new()
    }
}
