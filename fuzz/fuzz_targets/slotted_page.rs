#![no_main]

use libfuzzer_sys::fuzz_target;
use task_pool::storage::slotted_page::{SlotId, SlottedPage, PAGE_SIZE};

const HEADER_SIZE: usize = 4;
const SLOT_SIZE: usize = 4;
const MAX_RECORD_SIZE: usize = 128;

fn check_invariants(page: &SlottedPage) {
    let slot_count = page.get_slot_count();
    let free_end = page.free_end() as usize;
    let free_space = page.free_space() as usize;

    let slot_directory_end = HEADER_SIZE + slot_count as usize * SLOT_SIZE;

    assert!(slot_directory_end <= PAGE_SIZE);
    assert!(free_end <= PAGE_SIZE);
    assert!(slot_directory_end <= free_end);

    let expected_free_space = free_end - slot_directory_end;

    assert_eq!(free_space, expected_free_space);

    for slot in 0..slot_count {
        let slot = slot as SlotId;

        let record = page.get(slot);

        if !record.is_empty() {
            assert!(!record.is_empty());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut page = SlottedPage::new();

    if data.is_empty() {
        return;
    }

    let mut cursor = 0;

    while cursor < data.len() {
        let op = data[cursor] % 3;
        cursor += 1;

        match op {
            0 => {
                if cursor >= data.len() {
                    break;
                }

                let length = (data[cursor] as usize % MAX_RECORD_SIZE) + 1;
                cursor += 1;

                let end = (cursor + length).min(data.len());

                if cursor >= end {
                    break;
                }

                let record = &data[cursor..end];
                cursor = end;

                let _ = page.insert(record);
            }

            1 => {
                let slot_count = page.get_slot_count();

                if slot_count != 0 {
                    let slot = data[cursor % data.len()] as SlotId % slot_count;

                    let _ = page.delete(slot);
                }

                cursor += 1;
            }

            2 => {
                page.compact();
            }

            _ => unreachable!(),
        }

        check_invariants(&page);
    }
});
