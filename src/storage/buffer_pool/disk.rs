use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::storage::buffer_pool::frame::PageId;
use crate::storage::PAGE_SIZE;

pub struct DiskManager {
    file: File,
}

impl DiskManager {
    pub fn new(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        Ok(Self { file })
    }

    pub fn read_page(&mut self, page_id: PageId, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffer size must equal PAGE_SIZE",
            ));
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;

        self.file.seek(SeekFrom::Start(offset))?;

        buf.fill(0);

        let mut total_read = 0;

        while total_read < PAGE_SIZE {
            let n = self.file.read(&mut buf[total_read..])?;

            if n == 0 {
                break;
            }

            total_read += n;
        }

        Ok(())
    }

    pub fn write_page(&mut self, page_id: PageId, buf: &[u8]) -> io::Result<()> {
        if buf.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffer size must equal PAGE_SIZE",
            ));
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;

        self.file.seek(SeekFrom::Start(offset))?;

        self.file.write_all(buf)?;

        self.file.sync_all()?;

        Ok(())
    }
}
