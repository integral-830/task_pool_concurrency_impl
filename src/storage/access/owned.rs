use crate::storage::access::iterator::TableIterator;

use crate::storage::heap::scan::HeapFileScan;

pub struct OwnedAdapter<'heap> {
    inner: HeapFileScan<'heap>,
}

impl<'heap> OwnedAdapter<'heap> {
    pub fn new(inner: HeapFileScan<'heap>) -> Self {
        Self { inner }
    }
}

impl Iterator for OwnedAdapter<'_> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|record| record.to_vec())
    }
}
#[cfg(test)]
mod owned_adapter_tests {
    use super::*;
    use crate::storage::heap::HeapFile;

    #[test]
    fn test_owned_adapter() {
        let mut heap_file = HeapFile::new(8).unwrap();

        heap_file.insert(b"record-a").unwrap();
        heap_file.insert(b"record-b").unwrap();

        let records: Vec<Vec<u8>> = OwnedAdapter::new(heap_file.scan()).collect();

        assert_eq!(records, vec![b"record-a".to_vec(), b"record-b".to_vec(),]);
    }
}
