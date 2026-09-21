use crate::storage::access::iterator::TableIterator;
use crate::storage::buffer_pool::frame::PageId;
use crate::storage::heap::HeapFile;
use crate::storage::page::slotted_page::SlotId;

pub struct HeapFileScan<'heap> {
    heap_file: &'heap HeapFile,
    page_id: PageId,
    slot_id: SlotId,
}

impl<'heap> HeapFileScan<'heap> {
    pub fn new(heap_file: &'heap HeapFile) -> Self {
        Self {
            heap_file,
            page_id: 0,
            slot_id: 0,
        }
    }
}

impl TableIterator for HeapFileScan<'_> {
    type Item<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        loop {
            let page_id = self.page_id as usize;
            let slot_id = self.slot_id;
            let total_pages = self.heap_file.pages.len();
            if page_id >= total_pages {
                return None;
            }
            let page = self.heap_file.get_page(page_id as PageId).ok()?;
            if slot_id >= page.get_slot_count() {
                self.page_id += 1;
                self.slot_id = 0;
                continue;
            }
            self.slot_id += 1;
            let data = page.get(slot_id);
            if data.is_empty() {
                continue;
            }
            return Some(data);
        }
    }
}

#[cfg(test)]
pub mod heap_scan_tests {
    use crate::storage::access::iterator::TableIterator;
    use crate::storage::heap::scan::HeapFileScan;
    use crate::storage::heap::HeapFile;

    #[test]
    fn test_empty_heap_scan() {
        let heap_file = HeapFile::new(8).unwrap();
        let mut scan = HeapFileScan::new(&heap_file);

        assert!(scan.next().is_none());
    }
    #[test]
    fn test_scan_one_record() {
        let mut heap_file = HeapFile::new(8).unwrap();

        heap_file.insert(b"hello").unwrap();

        let mut scan = HeapFileScan::new(&heap_file);

        assert_eq!(scan.next(), Some(b"hello".as_slice()));
        assert_eq!(scan.next(), None);
    }
    #[test]
    fn test_scan_multiple_records() {
        let mut heap_file = HeapFile::new(8).unwrap();

        heap_file.insert(b"hello").unwrap();
        heap_file.insert(b"world").unwrap();
        heap_file.insert(b"rust").unwrap();

        let mut scan = HeapFileScan::new(&heap_file);

        assert_eq!(scan.next(), Some(b"hello".as_slice()));
        assert_eq!(scan.next(), Some(b"world".as_slice()));
        assert_eq!(scan.next(), Some(b"rust".as_slice()));
        assert_eq!(scan.next(), None);
    }
    #[test]
    fn test_scan_skips_deleted_slots() {
        let mut heap_file = HeapFile::new(8).unwrap();

        let page_id = heap_file.create_page().unwrap();

        heap_file.insert_page(page_id, b"hello").unwrap();
        heap_file.insert_page(page_id, b"world").unwrap();
        heap_file.insert_page(page_id, b"rust").unwrap();

        heap_file.get_page_mut(page_id).unwrap().delete(1).unwrap();

        let mut scan = HeapFileScan::new(&heap_file);

        assert_eq!(scan.next(), Some(b"hello".as_slice()));
        assert_eq!(scan.next(), Some(b"rust".as_slice()));
        assert_eq!(scan.next(), None);
    }
    #[test]
    fn test_scan_multiple_pages() {
        let mut heap_file = HeapFile::new(8).unwrap();

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();

        heap_file.insert_page(page0, b"page0-record").unwrap();
        heap_file.insert_page(page1, b"page1-record").unwrap();

        let mut scan = HeapFileScan::new(&heap_file);

        assert_eq!(scan.next(), Some(b"page0-record".as_slice()));

        assert_eq!(scan.next(), Some(b"page1-record".as_slice()));

        assert_eq!(scan.next(), None);
    }
}
