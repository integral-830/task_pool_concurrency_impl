use std::io;

use crate::storage::PAGE_SIZE;

pub type SlotId = u16;
const HEADER_SIZE: usize = 4;
pub const SLOT_SIZE: usize = 4;

pub const MAX_AVAILABLE_SLOTTED_PAGE_SPACE: usize = PAGE_SIZE - (HEADER_SIZE);

pub struct SlottedPage {
    data: [u8; PAGE_SIZE],
}

impl SlottedPage {
    pub fn new() -> Self {
        let mut data = [0u8; PAGE_SIZE];
        data[2..4].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
        Self { data }
    }

    pub fn get_slot_count(&self) -> u16 {
        u16::from_le_bytes(self.data[0..2].try_into().unwrap())
    }

    pub fn free_end(&self) -> u16 {
        u16::from_le_bytes(self.data[2..4].try_into().unwrap())
    }

    pub fn free_space(&self) -> u16 {
        let slot_count = self.get_slot_count();
        let free_end = self.free_end();
        free_end - (slot_count * SLOT_SIZE as u16 + HEADER_SIZE as u16)
    }

    fn read_slot(&self, slot: SlotId) -> (u16, u16) {
        let slot_offset = HEADER_SIZE + slot as usize * SLOT_SIZE;
        let tuple_offset =
            u16::from_le_bytes(self.data[slot_offset..slot_offset + 2].try_into().unwrap());
        let tuple_length = u16::from_le_bytes(
            self.data[slot_offset + 2..slot_offset + 4]
                .try_into()
                .unwrap(),
        );
        (tuple_offset, tuple_length)
    }

    fn write_slot(&mut self, slot: SlotId, offset: u16, length: u16) {
        let slot_offset = HEADER_SIZE + slot as usize * SLOT_SIZE;
        self.data[slot_offset..slot_offset + 2].copy_from_slice(&offset.to_le_bytes());
        self.data[slot_offset + 2..slot_offset + 4].copy_from_slice(&length.to_le_bytes());
    }

    pub fn insert(&mut self, record: &[u8]) -> io::Result<SlotId> {
        let required_size = record.len() + SLOT_SIZE;
        let free_size = self.free_space() as usize;
        if free_size < required_size {
            return Err(io::Error::new(io::ErrorKind::Other, "Out of memory..."));
        }
        let free_end = self.free_end() as usize;
        let tuple_offset = free_end - record.len();
        self.data[tuple_offset..free_end].copy_from_slice(record);
        self.data[2..4].copy_from_slice(&((tuple_offset as u16).to_le_bytes()));
        let slot_count = self.get_slot_count();
        self.data[0..2].copy_from_slice(&(slot_count + 1).to_le_bytes());
        self.write_slot(slot_count, tuple_offset as u16, record.len() as u16);
        Ok(slot_count)
    }
    pub fn get(&self, slot: SlotId) -> &[u8] {
        let (tuple_offset, tuple_length) = self.read_slot(slot);
        &self.data[tuple_offset as usize..(tuple_offset + tuple_length) as usize]
    }
    pub fn delete(&mut self, slot: SlotId) -> io::Result<()> {
        let slot_count = self.get_slot_count();
        if slot >= slot_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Slot doesn't exist...",
            ));
        }
        self.write_slot(slot, 0, 0);
        Ok(())
    }

    pub fn compact(&mut self) {
        let slot_count = self.get_slot_count();
        let mut destination = HEADER_SIZE + SLOT_SIZE * slot_count as usize;
        for i in 0..slot_count {
            let (offset, length) = self.read_slot(i);
            if length == 0 {
                continue;
            }
            let mut source = vec![0u8; length as usize];
            source.copy_from_slice(&self.data[offset as usize..offset as usize + length as usize]);
            self.data[destination..destination + length as usize].copy_from_slice(&source);
            self.write_slot(i, destination as u16, length);
            destination += length as usize;
        }
        self.data[2..4].copy_from_slice(&(destination as u16).to_le_bytes());
    }
}

impl Default for SlottedPage {
    fn default() -> Self {
        Self::new()
    }
}
