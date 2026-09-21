pub mod scan;
use std::collections::HashSet;

use crate::storage::fsm::tree::{FreeSpaceMap, FreeSpaceMapError};
use crate::storage::heap::scan::HeapFileScan;

use super::access::filter::FilterScan;
use super::access::owned::OwnedAdapter;
use super::buffer_pool::frame::PageId;
use super::page::slotted_page::{SlottedPage, MAX_AVAILABLE_SLOTTED_PAGE_SPACE, SLOT_SIZE};

pub struct HeapFile {
    pages: Vec<SlottedPage>,
    fsm: FreeSpaceMap,
    capacity: usize,
}

#[derive(Debug)]
pub enum HeapFileError {
    InvalidPageId,
    FreeSpaceMapError(FreeSpaceMapError),
    HeapFileFull,
    SlottedPageError(std::io::Error),
    NoPageFoundForInsertion,
    InsufficientPageSpace,
}

impl HeapFile {
    pub fn new(capacity: usize) -> Result<Self, HeapFileError> {
        Ok(Self {
            pages: Vec::new(),
            fsm: FreeSpaceMap::new(capacity).map_err(HeapFileError::FreeSpaceMapError)?,
            capacity,
        })
    }

    pub fn create_page(&mut self) -> Result<PageId, HeapFileError> {
        if self.pages.len() >= self.capacity {
            return Err(HeapFileError::HeapFileFull);
        }
        let page_id = self.pages.len() as PageId;
        let slotted_page = SlottedPage::new();
        let free_space = slotted_page.free_space();
        self.fsm
            .register_page(page_id, free_space as usize)
            .map_err(HeapFileError::FreeSpaceMapError)?;
        self.pages.push(slotted_page);
        Ok(page_id)
    }

    fn insert_page(&mut self, page_id: PageId, record: &[u8]) -> Result<(), HeapFileError> {
        let page = self.get_page_mut(page_id)?;
        page.insert(record)
            .map_err(HeapFileError::SlottedPageError)?;
        let free_space = page.free_space();
        self.fsm
            .update_page(page_id, free_space as usize)
            .map_err(HeapFileError::FreeSpaceMapError)?;
        Ok(())
    }

    pub fn insert(&mut self, record: &[u8]) -> Result<(), HeapFileError> {
        let req_space = record.len() + SLOT_SIZE;

        if req_space > MAX_AVAILABLE_SLOTTED_PAGE_SPACE {
            return Err(HeapFileError::InsufficientPageSpace);
        }

        let mut excluded_page_ids = HashSet::new();

        loop {
            let page_id = self
                .fsm
                .find_page_excluding(req_space, &excluded_page_ids)
                .map_err(HeapFileError::FreeSpaceMapError)?;

            let Some(page_id) = page_id else {
                let page_id = self.create_page()?;
                self.insert_page(page_id, record)?;
                return Ok(());
            };

            let free_space = self.get_page(page_id)?.free_space() as usize;

            if free_space >= req_space {
                self.insert_page(page_id, record)?;
                return Ok(());
            }

            excluded_page_ids.insert(page_id);
        }
    }

    pub fn scan(&self) -> HeapFileScan<'_> {
        HeapFileScan::new(self)
    }

    pub fn scan_owned(&self) -> OwnedAdapter {
        OwnedAdapter::new(self.scan())
    }

    pub fn get_page(&self, page_id: PageId) -> Result<&SlottedPage, HeapFileError> {
        if page_id as usize >= self.pages.len() {
            return Err(HeapFileError::InvalidPageId);
        }
        Ok(&self.pages[page_id as usize])
    }

    pub fn get_page_mut(&mut self, page_id: PageId) -> Result<&mut SlottedPage, HeapFileError> {
        if page_id as usize >= self.pages.len() {
            return Err(HeapFileError::InvalidPageId);
        }
        Ok(&mut self.pages[page_id as usize])
    }
}

#[cfg(test)]
pub mod heap_file_tests {
    use crate::storage::{
        buffer_pool::frame::PageId,
        heap::{HeapFile, HeapFileError},
    };

    #[test]
    fn test_create_first_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        assert!(page_id == 0);
        assert!(heap_file.pages.len() == 1);
    }

    #[test]
    fn test_create_multiple_pages() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        assert!(heap_file.create_page().expect("create_page error...") == 0);
        assert!(heap_file.create_page().expect("Initialization error...") == 1);
        assert!(heap_file.pages.len() == 2);
        assert!(heap_file.create_page().expect("Initialization error...") == 2);
        assert!(heap_file.pages.len() == 3);
        assert!(heap_file.create_page().expect("Initialization error...") == 3);
        assert!(heap_file.pages.len() == 4);
        assert!(heap_file.create_page().expect("Initialization error...") == 4);
        assert!(heap_file.pages.len() == 5);
    }

    #[test]
    fn test_get_invalid_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        let page = heap_file.get_page(1);
        assert!(page_id == 0);
        assert!(page.is_err());
    }

    #[test]
    fn test_correct_bucket_mapping() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        let page = heap_file.get_page(page_id).expect("get page failed...");
        let actual_free_space = page.free_space();
        let expected_bucket = heap_file
            .fsm
            .get_bucket_id(actual_free_space as usize)
            .expect("get_bucket_id failed...");
        assert!(
            heap_file
                .fsm
                .get_leaf_bucket(page_id)
                .expect("get_leaf_bucket failed...")
                == Some(expected_bucket)
        );
        assert!(page_id == 0);
    }

    #[test]
    fn test_insert_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        heap_file
            .insert_page(page_id, b"hello")
            .expect("insert_page failed...");
        assert!(page_id == 0);
        assert!(
            heap_file
                .get_page(page_id)
                .expect("get page failed...")
                .get(0)
                == b"hello"
        );
    }

    #[test]
    fn test_correct_free_space_remaining() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        let old_free_space = heap_file
            .get_page(page_id)
            .expect("get page failed...")
            .free_space();
        heap_file
            .insert_page(page_id, b"hello")
            .expect("insertion failed...");
        let new_free_space = heap_file
            .get_page(page_id)
            .expect("get page error...")
            .free_space();

        assert!(
            heap_file
                .get_page(page_id)
                .expect("get page failed...")
                .get(0)
                == b"hello"
        );
        assert!(old_free_space > new_free_space);
    }

    #[test]
    fn test_large_data_fails_insertion() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        let slot_id = heap_file.insert_page(page_id, &[0u8; 4097]);
        let old_free_space = heap_file
            .get_page(page_id)
            .expect("get page failed...")
            .free_space();
        assert!(slot_id.is_err());
        assert!(old_free_space == 4092);
    }

    #[test]
    fn test_insert() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create_page error...");
        heap_file.insert(b"hello").expect("insert_page failed...");
        assert!(page_id == 0);
        assert!(
            heap_file
                .get_page(page_id)
                .expect("get page failed...")
                .get(0)
                == b"hello"
        );
    }

    #[test]
    fn test_insert_selects_existing_page_with_enough_space() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page0 = heap_file.create_page().expect("create page 0 failed...");
        let page1 = heap_file.create_page().expect("create page 1 failed...");

        heap_file
            .insert_page(page0, &[0u8; 3000])
            .expect("insert page 0 failed...");

        heap_file
            .insert(b"hello")
            .expect("automatic insertion failed...");

        assert!(heap_file.get_page(page0).unwrap().get(1) == b"hello");
        assert!(heap_file.get_page(page1).unwrap().get(0).is_empty());
    }

    #[test]
    fn test_insert_selects_leftmost_qualifying_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page0 = heap_file.create_page().expect("create page 0 failed...");
        let page1 = heap_file.create_page().expect("create page 1 failed...");

        heap_file
            .insert_page(page0, &[0u8; 3000])
            .expect("insert page 0 failed...");

        let page0_before = heap_file.get_page(page0).unwrap().free_space();
        let page1_before = heap_file.get_page(page1).unwrap().free_space();

        heap_file
            .insert(b"hello")
            .expect("automatic insertion failed...");

        let page0_after = heap_file.get_page(page0).unwrap().free_space();
        let page1_after = heap_file.get_page(page1).unwrap().free_space();

        assert!(page0_after < page0_before);
        assert_eq!(page1_after, page1_before);
    }

    #[test]
    fn test_automatic_insert_updates_fsm() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create page failed...");

        let old_free_space = heap_file.get_page(page_id).unwrap().free_space();

        let old_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        heap_file
            .insert(b"hello")
            .expect("automatic insertion failed...");

        let new_free_space = heap_file.get_page(page_id).unwrap().free_space();

        let new_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        let expected_bucket = heap_file
            .fsm
            .get_bucket_id(new_free_space as usize)
            .unwrap();

        assert!(new_free_space < old_free_space);
        assert_eq!(new_bucket, expected_bucket);
        assert!(new_bucket <= old_bucket);
    }

    #[test]
    fn test_multiple_inserts_keep_fsm_synchronized() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create page failed...");

        for _ in 0..20 {
            heap_file
                .insert(b"hello world")
                .expect("automatic insertion failed...");

            let page = heap_file.get_page(page_id).unwrap();

            let actual_bucket = heap_file
                .fsm
                .get_bucket_id(page.free_space() as usize)
                .unwrap();

            let fsm_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

            assert_eq!(fsm_bucket, actual_bucket);
        }
    }

    #[test]
    fn test_insert_page_invalid_page_id() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        let result = heap_file.insert_page(0, b"hello");

        assert!(result.is_err());
    }

    #[test]
    fn test_failed_insert_preserves_fsm() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create page failed...");

        let old_free_space = heap_file.get_page(page_id).unwrap().free_space();

        let old_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        let result = heap_file.insert_page(page_id, &[0u8; 4097]);

        assert!(result.is_err());

        let new_free_space = heap_file.get_page(page_id).unwrap().free_space();

        let new_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        assert_eq!(old_free_space, new_free_space);
        assert_eq!(old_bucket, new_bucket);
    }

    #[test]
    fn test_insert_crosses_bucket_boundary() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");
        let page_id = heap_file.create_page().expect("create page failed...");

        loop {
            let free_space = heap_file.get_page(page_id).unwrap().free_space();

            if free_space < 1024 {
                break;
            }

            heap_file
                .insert_page(page_id, &[0u8; 100])
                .expect("insert failed...");
        }

        let page = heap_file.get_page(page_id).unwrap();

        let actual_bucket = heap_file
            .fsm
            .get_bucket_id(page.free_space() as usize)
            .unwrap();

        let fsm_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        assert_eq!(fsm_bucket, actual_bucket);
        assert!(actual_bucket <= 1);
    }

    #[test]
    fn test_automatic_insert_uses_qualifying_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();

        heap_file.insert_page(page0, &[0u8; 3000]).unwrap();

        let page0_before = heap_file.get_page(page0).unwrap().free_space();
        let page1_before = heap_file.get_page(page1).unwrap().free_space();

        heap_file.insert(&[1u8; 100]).unwrap();

        let page0_after = heap_file.get_page(page0).unwrap().free_space();
        let page1_after = heap_file.get_page(page1).unwrap().free_space();

        assert!(page0_after < page0_before);
        assert_eq!(page1_after, page1_before);
    }

    #[test]
    fn test_all_pages_fsm_consistency() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        for _ in 0..8 {
            heap_file.create_page().unwrap();
        }

        for page_id in 0..8 {
            let page_id = page_id as PageId;

            let page = heap_file.get_page(page_id).unwrap();

            let expected_bucket = heap_file
                .fsm
                .get_bucket_id(page.free_space() as usize)
                .unwrap();

            let actual_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

            assert_eq!(actual_bucket, expected_bucket);
        }
    }
    #[test]
    fn test_insert_creates_first_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        assert_eq!(heap_file.pages.len(), 0);

        heap_file.insert(b"hello").expect("insert failed...");

        assert_eq!(heap_file.pages.len(), 1);
        assert_eq!(heap_file.get_page(0).unwrap().get(0), b"hello");
    }
    #[test]
    fn test_insert_reuses_existing_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        heap_file.insert(b"hello").expect("first insert failed...");
        heap_file.insert(b"world").expect("second insert failed...");

        assert_eq!(heap_file.pages.len(), 1);
        assert_eq!(heap_file.get_page(0).unwrap().get(0), b"hello");
        assert_eq!(heap_file.get_page(0).unwrap().get(1), b"world");
    }
    #[test]
    fn test_insert_allocates_new_page_when_existing_page_cannot_fit() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        heap_file
            .insert(&[0u8; 3000])
            .expect("first insert failed...");

        assert_eq!(heap_file.pages.len(), 1);

        heap_file
            .insert(&[1u8; 2500])
            .expect("second insert failed...");

        assert_eq!(heap_file.pages.len(), 2);
    }

    #[test]
    fn test_newly_allocated_page_contains_record() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        heap_file
            .insert(&[0u8; 3000])
            .expect("first insert failed...");

        heap_file
            .insert(&[1u8; 2500])
            .expect("second insert failed...");

        assert_eq!(heap_file.pages.len(), 2);
        assert_eq!(heap_file.get_page(1).unwrap().get(0), &[1u8; 2500]);
    }

    #[test]
    fn test_new_page_is_registered_in_fsm() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        heap_file
            .insert(&[0u8; 3000])
            .expect("first insert failed...");

        heap_file
            .insert(&[1u8; 2500])
            .expect("second insert failed...");

        let page_id = 1;

        let page = heap_file.get_page(page_id).unwrap();

        let expected_bucket = heap_file
            .fsm
            .get_bucket_id(page.free_space() as usize)
            .unwrap();

        let actual_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

        assert_eq!(actual_bucket, expected_bucket);
    }

    #[test]
    fn test_heap_file_full() {
        let mut heap_file = HeapFile::new(1).expect("Initialization error...");

        heap_file
            .insert(&[0u8; 3000])
            .expect("first insert failed...");

        let result = heap_file.insert(&[1u8; 2500]);

        assert!(matches!(result, Err(HeapFileError::HeapFileFull)));
        assert_eq!(heap_file.pages.len(), 1);
    }

    #[test]
    fn test_oversized_record_does_not_create_page() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        let result = heap_file.insert(&[0u8; 4097]);

        assert!(matches!(result, Err(HeapFileError::InsufficientPageSpace)));

        assert_eq!(heap_file.pages.len(), 0);
    }

    #[test]
    fn test_fsm_consistency_after_automatic_allocation() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        heap_file
            .insert(&[0u8; 3000])
            .expect("first insert failed...");

        heap_file
            .insert(&[1u8; 2500])
            .expect("second insert failed...");

        for page_id in 0..heap_file.pages.len() {
            let page_id = page_id as PageId;

            let page = heap_file.get_page(page_id).unwrap();

            let expected_bucket = heap_file
                .fsm
                .get_bucket_id(page.free_space() as usize)
                .unwrap();

            let actual_bucket = heap_file.fsm.get_leaf_bucket(page_id).unwrap().unwrap();

            assert_eq!(actual_bucket, expected_bucket);
        }
    }

    #[test]
    fn test_multiple_automatic_page_allocations() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        for _ in 0..8 {
            heap_file.insert(&[0u8; 4000]).expect("insert failed...");
        }

        assert_eq!(heap_file.pages.len(), 8);
    }

    #[test]
    fn test_insert_retries_after_fsm_false_positive() {
        let mut heap_file = HeapFile::new(8).expect("Initialization error...");

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();

        heap_file
            .insert_page(page0, &[0u8; 3492])
            .expect("insert into page 0 failed...");

        let page0_free = heap_file.get_page(page0).unwrap().free_space();
        let page1_free = heap_file.get_page(page1).unwrap().free_space();

        assert_eq!(page0_free, 596);
        assert_eq!(page1_free, 4092);

        heap_file
            .insert(&[1u8; 696])
            .expect("insert should retry after false positive...");

        assert_eq!(heap_file.get_page(page0).unwrap().free_space(), page0_free);

        assert!(heap_file.get_page(page1).unwrap().free_space() < page1_free);

        assert_eq!(heap_file.get_page(page1).unwrap().get(0), &[1u8; 696]);
    }

    #[test]
    fn test_false_positive_does_not_modify_page_or_fsm() {
        let mut heap_file = HeapFile::new(8).unwrap();

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();

        heap_file.insert_page(page0, &[0u8; 3492]).unwrap();

        let free_before = heap_file.get_page(page0).unwrap().free_space();
        let bucket_before = heap_file.fsm.get_leaf_bucket(page0).unwrap().unwrap();

        heap_file.insert(&[1u8; 696]).unwrap();

        let free_after = heap_file.get_page(page0).unwrap().free_space();
        let bucket_after = heap_file.fsm.get_leaf_bucket(page0).unwrap().unwrap();

        assert_eq!(free_before, 596);
        assert_eq!(free_before, free_after);
        assert_eq!(bucket_before, bucket_after);

        assert_eq!(heap_file.get_page(page1).unwrap().get(0), &[1u8; 696]);
    }
    #[test]
    fn test_insert_retries_multiple_false_positives() {
        let mut heap_file = HeapFile::new(8).unwrap();

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();
        let page2 = heap_file.create_page().unwrap();

        heap_file.insert_page(page0, &[0u8; 3492]).unwrap();

        heap_file.insert_page(page1, &[0u8; 3492]).unwrap();

        heap_file
            .insert(&[1u8; 696])
            .expect("insert should reach page 2");

        assert_eq!(heap_file.get_page(page0).unwrap().free_space(), 596);

        assert_eq!(heap_file.get_page(page1).unwrap().free_space(), 596);

        assert_eq!(heap_file.get_page(page2).unwrap().get(0), &[1u8; 696]);
    }
    #[test]
    fn test_insert_allocates_after_all_candidates_fail() {
        let mut heap_file = HeapFile::new(8).unwrap();

        let page0 = heap_file.create_page().unwrap();
        let page1 = heap_file.create_page().unwrap();

        heap_file.insert_page(page0, &[0u8; 3492]).unwrap();

        heap_file.insert_page(page1, &[0u8; 3492]).unwrap();

        assert_eq!(heap_file.pages.len(), 2);

        heap_file.insert(&[1u8; 696]).unwrap();

        assert_eq!(heap_file.pages.len(), 3);
        assert_eq!(heap_file.get_page(2).unwrap().get(0), &[1u8; 696]);
    }

    pub fn test_scan_iterator_init() {
        let mut heap_file = HeapFile::new(8).expect("heap_file init error...");

        heap_file.insert(b"hello").unwrap();
        heap_file.insert(b"world").unwrap();

        let mut scan = heap_file.scan();
    }
}
