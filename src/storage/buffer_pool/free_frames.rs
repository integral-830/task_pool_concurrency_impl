use std::{collections::VecDeque, sync::Mutex};

use super::clock_replacer::FrameId;

pub struct FreeFrames {
    frames: Mutex<VecDeque<FrameId>>,
}

#[derive(Debug)]
pub enum FreeFramesError {
    LockPoisoned,
}

impl FreeFrames {
    pub fn new() -> Self {
        Self {
            frames: Mutex::new(VecDeque::new()),
        }
    }

    pub fn initialize(size: usize) -> Result<Self, FreeFramesError> {
        let init = Self::new();
        let mut lock = init
            .frames
            .lock()
            .map_err(|_| FreeFramesError::LockPoisoned)?;
        for i in 0..size {
            lock.push_back(i);
        }
        drop(lock);
        Ok(init)
    }

    pub fn add_frame(&self, frame_id: FrameId) -> Result<(), FreeFramesError> {
        self.frames
            .lock()
            .map_err(|_| FreeFramesError::LockPoisoned)?
            .push_back(frame_id);
        Ok(())
    }

    pub fn get_frame(&self) -> Result<Option<FrameId>, FreeFramesError> {
        let frame = self
            .frames
            .lock()
            .map_err(|_| FreeFramesError::LockPoisoned)?
            .pop_front();
        Ok(frame)
    }

    pub fn len(&self) -> Result<usize, FreeFramesError> {
        let len = self
            .frames
            .lock()
            .map_err(|_| FreeFramesError::LockPoisoned)?
            .len();
        Ok(len)
    }

    pub fn is_empty(&self) -> Result<bool, FreeFramesError> {
        let bool = self
            .frames
            .lock()
            .map_err(|_| FreeFramesError::LockPoisoned)?
            .is_empty();
        Ok(bool)
    }
}

impl Default for FreeFrames {
    fn default() -> Self {
        Self::new()
    }
}
