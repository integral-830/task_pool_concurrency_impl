use std::collections::HashMap;
use std::sync::RwLock;

use super::clock_replacer::FrameId;
use super::frame::PageId;

pub struct PageTable {
    table: RwLock<HashMap<PageId, FrameId>>,
}

#[derive(Debug)]
pub enum PageTableError {
    LockPoisoned,
    PoisonedMapping,
    OldPageMissing,
    PageAlreadyExist,
}

impl PageTable {
    pub fn new() -> Self {
        Self {
            table: RwLock::new(HashMap::new()),
        }
    }

    pub fn insert(&self, page_id: PageId, frame_id: FrameId) -> Result<(), PageTableError> {
        self.table
            .write()
            .map_err(|_| PageTableError::LockPoisoned)?
            .insert(page_id, frame_id);
        Ok(())
    }

    pub fn replace(
        &self,
        old_page_id: PageId,
        new_page_id: PageId,
        frame_id: FrameId,
    ) -> Result<(), PageTableError> {
        let mut table = self
            .table
            .write()
            .map_err(|_| PageTableError::LockPoisoned)?;
        if let Some(old_map) = table.get(&old_page_id) {
            if old_map == &frame_id {
                if !table.contains_key(&new_page_id) {
                    table.remove(&old_page_id);
                    table.insert(new_page_id, frame_id);
                    return Ok(());
                }
                return Err(PageTableError::PageAlreadyExist);
            }
            return Err(PageTableError::PoisonedMapping);
        }
        Err(PageTableError::OldPageMissing)
    }

    pub fn get_frame_id(&self, page_id: PageId) -> Result<Option<FrameId>, PageTableError> {
        if let Some(&frame) = self
            .table
            .read()
            .map_err(|_| PageTableError::LockPoisoned)?
            .get(&page_id)
        {
            return Ok(Some(frame));
        }
        Ok(None)
    }

    pub fn get_resident_ids(&self) -> Result<Vec<PageId>, PageTableError> {
        let mut ids = Vec::<PageId>::new();
        for (key, _) in self
            .table
            .read()
            .map_err(|_| PageTableError::LockPoisoned)?
            .iter()
        {
            ids.push(*key);
        }
        Ok(ids)
    }

    pub fn contains(&self, page_id: PageId) -> Result<bool, PageTableError> {
        let result = self
            .table
            .read()
            .map_err(|_| PageTableError::LockPoisoned)?
            .contains_key(&page_id);
        Ok(result)
    }

    pub fn remove_page(&self, page_id: PageId) -> Result<(), PageTableError> {
        self.table
            .write()
            .map_err(|_| PageTableError::LockPoisoned)?
            .remove(&page_id);
        Ok(())
    }

    pub fn len(&self) -> Result<usize, PageTableError> {
        let len = self
            .table
            .read()
            .map_err(|_| PageTableError::LockPoisoned)?
            .len();
        Ok(len)
    }
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}
