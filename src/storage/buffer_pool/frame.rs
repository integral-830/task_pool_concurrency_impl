use std::io;

use crate::storage::PAGE_SIZE;

pub type PageId = u16;

pub struct PageMeta {
    pub page_id: PageId,
    pin_count: u8,
    pub dirty: bool,
    reference: bool,
}

pub struct Frame {
    pub data: [u8; PAGE_SIZE],
    pub metadata: Option<PageMeta>,
    pub state: FrameState,
}

#[derive(Debug, PartialEq)]
pub enum FrameState {
    Free,
    Reserved,
    Resident,
}

impl Frame {
    pub fn new() -> Self {
        Self {
            data: [0u8; PAGE_SIZE],
            metadata: None,
            state: FrameState::Free,
        }
    }

    pub fn load_page(&mut self, page_id: PageId, data: &[u8]) -> io::Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid data length...",
            ));
        }
        self.data.copy_from_slice(data);
        self.metadata = Some(PageMeta {
            page_id,
            pin_count: 0,
            dirty: false,
            reference: false,
        });
        self.set_resident()?;
        Ok(())
    }

    pub fn read_data(&self) -> &[u8] {
        &self.data
    }

    pub fn modify_data(&mut self, data: &[u8]) -> io::Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid data length...",
            ));
        }
        self.data.copy_from_slice(data);
        self.mark_dirty()?;
        Ok(())
    }

    pub fn is_free(&self) -> bool {
        self.state == FrameState::Free
    }

    pub fn set_free(&mut self) -> io::Result<()> {
        self.state = FrameState::Free;
        Ok(())
    }

    pub fn is_resident(&self) -> bool {
        self.state == FrameState::Resident
    }

    pub fn set_resident(&mut self) -> io::Result<()> {
        self.state = FrameState::Resident;
        Ok(())
    }

    pub fn is_reserved(&self) -> bool {
        self.state == FrameState::Reserved
    }

    pub fn reserve(&mut self) -> io::Result<()> {
        if self.state != FrameState::Reserved {
            self.state = FrameState::Reserved;
            return Ok(());
        }
        Err(io::Error::other("Frame is already reserved..."))
    }

    pub fn pin(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            meta.pin_count = meta.pin_count.saturating_add(1);
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn unpin(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            if meta.pin_count > 0 {
                meta.pin_count -= 1;
            } else {
                return Err(io::Error::other("Cannot unpin further..."));
            }
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn mark_dirty(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            meta.dirty = true;
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn clear_dirty(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            meta.dirty = false;
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn mark_referenced(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            meta.reference = true;
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn mark_unreferenced(&mut self) -> io::Result<()> {
        if let Some(meta) = self.metadata.as_mut() {
            meta.reference = false;
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."));
        }
        Ok(())
    }

    pub fn get_page_id(&self) -> io::Result<PageId> {
        if let Some(meta) = &self.metadata {
            Ok(meta.page_id)
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "Page not found..."))
        }
    }

    pub fn is_dirty(&self) -> bool {
        if let Some(meta) = &self.metadata {
            if meta.dirty {
                return true;
            }
        }
        false
    }

    pub fn is_referenced(&self) -> bool {
        if let Some(meta) = &self.metadata {
            if meta.reference {
                return true;
            }
        }
        false
    }

    pub fn is_pinned(&self) -> bool {
        if let Some(meta) = &self.metadata {
            if meta.pin_count > 0 {
                return true;
            }
        }
        false
    }

    pub fn reset(&mut self) -> io::Result<()> {
        self.data = [0u8; PAGE_SIZE];
        self.metadata = None;
        self.set_free()?;
        Ok(())
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}
