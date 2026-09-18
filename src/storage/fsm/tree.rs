use std::collections::HashSet;

use crate::storage::buffer_pool::frame::PageId;

pub type Bucket = u8;
pub const BUCKET_COUNT: usize = 8;

pub struct Node {
    max_bucket: Option<Bucket>,
    page_id: Option<PageId>,
}

pub struct FreeSpaceMap {
    nodes: Vec<Node>,
    capacity: usize,
}

#[derive(Debug)]
pub enum FreeSpaceMapError {
    InvalidArgument,
    InvalidFreeSpace,
    InvalidPageId,
    PageDoesNotExist,
}

impl FreeSpaceMap {
    pub fn new(capacity: usize) -> Result<Self, FreeSpaceMapError> {
        if capacity <= 0 {
            return Err(FreeSpaceMapError::InvalidArgument);
        }
        let total_nodes = 2 * capacity - 1;
        Ok(Self {
            nodes: (0..total_nodes)
                .map(|_| Node {
                    max_bucket: None,
                    page_id: None,
                })
                .collect(),
            capacity,
        })
    }

    pub fn register_page(
        &mut self,
        page_id: PageId,
        free_space: usize,
    ) -> Result<Bucket, FreeSpaceMapError> {
        if page_id as usize >= self.capacity {
            return Err(FreeSpaceMapError::InvalidPageId);
        }
        let req_bucket = self.get_bucket_id(free_space)?;
        let leaf_index = self.capacity - 1 + page_id as usize;
        self.nodes[leaf_index].max_bucket = Some(req_bucket);
        self.nodes[leaf_index].page_id = Some(page_id);
        if leaf_index != 0 {
            let parent = (leaf_index - 1) / 2;
            self.recompute_ancestors(parent);
        }
        Ok(req_bucket)
    }

    pub fn update_page(
        &mut self,
        page_id: PageId,
        free_space: usize,
    ) -> Result<Bucket, FreeSpaceMapError> {
        if page_id as usize >= self.capacity {
            return Err(FreeSpaceMapError::InvalidPageId);
        }
        let new_bucket = self.get_bucket_id(free_space)?;
        let leaf_index = self.capacity - 1 + page_id as usize;
        if self.nodes[leaf_index].page_id != Some(page_id) {
            return Err(FreeSpaceMapError::PageDoesNotExist);
        }
        self.nodes[leaf_index].max_bucket = Some(new_bucket);

        if leaf_index != 0 {
            let parent = (leaf_index - 1) / 2;
            self.recompute_ancestors(parent);
        }
        Ok(new_bucket)
    }

    fn find_leaf(&self, req_bucket: Bucket, node: usize) -> Option<PageId> {
        if node >= self.capacity - 1 {
            if self.nodes[node].max_bucket? >= req_bucket {
                return self.nodes[node].page_id;
            }
            return None;
        }

        let left = 2 * node + 1;
        let right = 2 * node + 2;

        if self.nodes[left]
            .max_bucket
            .is_some_and(|bucket| bucket >= req_bucket)
        {
            return self.find_leaf(req_bucket, left);
        }

        if self.nodes[right]
            .max_bucket
            .is_some_and(|bucket| bucket >= req_bucket)
        {
            return self.find_leaf(req_bucket, right);
        }

        None
    }

    pub fn find_page(&self, required_space: usize) -> Result<Option<PageId>, FreeSpaceMapError> {
        let excluded_page_ids = HashSet::new();

        self.find_page_excluding(required_space, &excluded_page_ids)
    }

    pub fn find_page_excluding(
        &self,
        required_space: usize,
        excluded_page_ids: &HashSet<PageId>,
    ) -> Result<Option<PageId>, FreeSpaceMapError> {
        let required_bucket = self.get_bucket_id(required_space)?;

        if self.nodes.is_empty() {
            return Ok(None);
        }

        self.find_page_recursive(0, required_bucket, excluded_page_ids)
    }

    fn find_page_recursive(
        &self,
        node_index: usize,
        required_bucket: Bucket,
        excluded_page_ids: &HashSet<PageId>,
    ) -> Result<Option<PageId>, FreeSpaceMapError> {
        let node = &self.nodes[node_index];

        if node.max_bucket < Some(required_bucket) {
            return Ok(None);
        }

        if node_index >= self.capacity - 1 {
            let Some(page_id) = node.page_id else {
                return Ok(None);
            };

            if excluded_page_ids.contains(&page_id) {
                return Ok(None);
            }

            return Ok(Some(page_id));
        }

        let left = 2 * node_index + 1;
        let right = 2 * node_index + 2;

        if left < self.nodes.len() {
            if let Some(page_id) =
                self.find_page_recursive(left, required_bucket, excluded_page_ids)?
            {
                return Ok(Some(page_id));
            }
        }

        if right < self.nodes.len() {
            if let Some(page_id) =
                self.find_page_recursive(right, required_bucket, excluded_page_ids)?
            {
                return Ok(Some(page_id));
            }
        }

        Ok(None)
    }

    fn recompute_ancestors(&mut self, internal_node: usize) {
        if internal_node > self.capacity - 1 {
            return;
        }
        let left = self.nodes[2 * internal_node + 1].max_bucket;
        let right = self.nodes[2 * internal_node + 2].max_bucket;
        if left > right {
            self.nodes[internal_node].max_bucket = left;
        } else {
            self.nodes[internal_node].max_bucket = right;
        }
        if internal_node != 0 {
            let parent = (internal_node - 1) / 2;
            self.recompute_ancestors(parent);
        }
    }

    pub fn get_leaf_bucket(&self, page_id: PageId) -> Result<Option<Bucket>, FreeSpaceMapError> {
        if page_id as usize >= self.capacity {
            return Err(FreeSpaceMapError::InvalidPageId);
        }
        let leaf_index = self.capacity - 1 + page_id as usize;
        if let Some(bucket) = self.nodes[leaf_index].max_bucket {
            return Ok(Some(bucket));
        }
        Ok(None)
    }

    pub fn get_len(&self) -> usize {
        self.nodes.len()
    }

    pub fn get_capacity(&self) -> usize {
        self.capacity
    }

    pub fn get_root(&self) -> &Node {
        &self.nodes[0]
    }

    pub fn get_bucket_id(&self, free_space: usize) -> Result<Bucket, FreeSpaceMapError> {
        match free_space {
            0..512 => Ok(0),
            512..1024 => Ok(1),
            1024..1536 => Ok(2),
            1536..2048 => Ok(3),
            2048..2560 => Ok(4),
            2560..3072 => Ok(5),
            3072..3584 => Ok(6),
            3584..4096 => Ok(7),
            _ => Err(FreeSpaceMapError::InvalidFreeSpace),
        }
    }
}

#[cfg(test)]
pub mod fsm_tests {
    use crate::storage::fsm::tree::{Bucket, FreeSpaceMap, FreeSpaceMapError};

    #[test]
    fn test_init() {
        let fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");
        assert!(fsm.get_len() == 15);
        assert!(fsm.get_capacity() == 8);
    }

    #[test]
    fn test_zero_invariant() {
        let result = FreeSpaceMap::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_bucketization() {
        let fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");
        assert_eq!(fsm.get_bucket_id(1026).unwrap(), 2);
        assert_eq!(fsm.get_bucket_id(2096).unwrap(), 4);
        assert_eq!(fsm.get_bucket_id(3011).unwrap(), 5);
        assert_eq!(fsm.get_bucket_id(511).unwrap(), 0);
        assert_eq!(fsm.get_bucket_id(512).unwrap(), 1);
        assert!(fsm.get_bucket_id(4096).is_err());
    }

    #[test]
    fn test_register_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");
        fsm.register_page(0, 511).expect("Failed register...");
        fsm.register_page(1, 581).expect("Failed register...");
        fsm.register_page(2, 1026).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(2));
        fsm.register_page(3, 1534).expect("Failed register...");
        fsm.register_page(4, 2055).expect("Failed register...");
        fsm.register_page(5, 3584).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(7));
    }

    #[test]
    fn test_update_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");
        fsm.register_page(0, 511).expect("Failed register...");
        fsm.register_page(1, 581).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(1));
        fsm.update_page(1, 1026).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(2));
        fsm.register_page(3, 1534).expect("Failed register...");
        fsm.register_page(4, 2055).expect("Failed register...");
        fsm.register_page(5, 3584).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(7));
        fsm.update_page(5, 112).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(4));
        assert!(fsm.update_page(6, 113).is_err());
    }

    #[test]
    fn test_find_page_exact_bucket() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 600).expect("Failed register...");
        fsm.register_page(2, 1100).expect("Failed register...");

        let page = fsm
            .find_page(1024)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 2);
    }

    #[test]
    fn test_find_page_higher_bucket_qualifies() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 3000).expect("Failed register...");

        let page = fsm
            .find_page(1536)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 0);
    }

    #[test]
    fn test_find_page_no_candidate() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 600).expect("Failed register...");

        let page = fsm.find_page(2048).expect("find_page failed...");

        assert_eq!(page, None);
    }

    #[test]
    fn test_find_page_empty_fsm() {
        let fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        let page = fsm.find_page(100).expect("find_page failed...");

        assert_eq!(page, None);
    }

    #[test]
    fn test_find_page_prunes_left_subtree() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 600).expect("Failed register...");

        fsm.register_page(4, 2560).expect("Failed register...");
        fsm.register_page(5, 2048).expect("Failed register...");

        let page = fsm
            .find_page(2048)
            .expect("find_page failed...")
            .expect("No page found...");

        assert!(page == 4 || page == 5);
    }

    #[test]
    fn test_find_page_prefers_left_subtree() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 2048).expect("Failed register...");
        fsm.register_page(4, 3072).expect("Failed register...");

        let page = fsm
            .find_page(2048)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 0);
    }

    #[test]
    fn test_find_page_higher_bucket_leaf() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 3584).expect("Failed register...");

        let page = fsm
            .find_page(100)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 0);
    }

    #[test]
    fn test_find_page_invalid_free_space() {
        let fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        let result = fsm.find_page(4096);

        assert!(matches!(result, Err(FreeSpaceMapError::InvalidFreeSpace)));
    }

    #[test]
    fn test_find_page_after_update() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 3000).expect("Failed register...");
        fsm.register_page(1, 100).expect("Failed register...");

        let page = fsm
            .find_page(2560)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 0);

        fsm.update_page(0, 100).expect("Failed update...");

        let page = fsm.find_page(2560).expect("find_page failed...");

        assert_eq!(page, None);
    }

    #[test]
    fn test_find_page_after_update_preserves_other_subtree() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 3000).expect("Failed register...");
        fsm.register_page(4, 3500).expect("Failed register...");

        fsm.update_page(0, 100).expect("Failed update...");

        let page = fsm
            .find_page(3072)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 4);
    }

    #[test]
    fn test_find_page_bucket_boundary() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 512).expect("Failed register...");
        fsm.register_page(1, 1024).expect("Failed register...");
        fsm.register_page(2, 1536).expect("Failed register...");

        assert_eq!(
            fsm.find_page(512)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );

        assert_eq!(
            fsm.find_page(1024)
                .expect("find_page failed...")
                .expect("No page found..."),
            1
        );

        assert_eq!(
            fsm.find_page(1536)
                .expect("find_page failed...")
                .expect("No page found..."),
            2
        );
    }

    #[test]
    fn test_find_page_after_multiple_updates() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 1000).expect("Failed register...");
        fsm.register_page(2, 2000).expect("Failed register...");
        fsm.register_page(3, 3000).expect("Failed register...");

        assert_eq!(
            fsm.find_page(2500)
                .expect("find_page failed...")
                .expect("No page found..."),
            3
        );

        fsm.update_page(3, 100).expect("Failed update...");

        assert_eq!(fsm.find_page(2500).expect("find_page failed..."), None);

        fsm.update_page(2, 3000).expect("Failed update...");

        let page = fsm
            .find_page(2500)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 2);
    }

    #[test]
    fn test_capacity_one() {
        let mut fsm = FreeSpaceMap::new(1).expect("Unexpected behaviour...");

        assert_eq!(fsm.get_len(), 1);
        assert_eq!(fsm.get_capacity(), 1);

        assert_eq!(fsm.register_page(0, 100).expect("Failed register..."), 0);

        assert_eq!(fsm.get_root().max_bucket, Some(0));

        assert_eq!(
            fsm.find_page(100)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );

        fsm.update_page(0, 3000).expect("Failed update...");

        assert_eq!(fsm.get_root().max_bucket, Some(5));

        assert_eq!(
            fsm.find_page(2500)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );
    }

    #[test]
    fn test_capacity_one_invalid_page() {
        let mut fsm = FreeSpaceMap::new(1).expect("Unexpected behaviour...");

        assert!(matches!(
            fsm.register_page(1, 100),
            Err(FreeSpaceMapError::InvalidPageId)
        ));

        assert!(matches!(
            fsm.update_page(1, 100),
            Err(FreeSpaceMapError::InvalidPageId)
        ));
    }

    #[test]
    fn test_capacity_two_propagation() {
        let mut fsm = FreeSpaceMap::new(2).expect("Unexpected behaviour...");

        fsm.register_page(0, 600).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(1));

        fsm.register_page(1, 3584).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(7));

        fsm.update_page(1, 100).expect("Failed update...");
        assert_eq!(fsm.get_root().max_bucket, Some(1));

        fsm.update_page(0, 100).expect("Failed update...");
        assert_eq!(fsm.get_root().max_bucket, Some(0));
    }

    #[test]
    fn test_all_pages_registration() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        for page_id in 0..8 {
            fsm.register_page(page_id, page_id as usize * 512)
                .expect("Failed register...");
        }

        assert_eq!(fsm.get_root().max_bucket, Some(7));
    }

    #[test]
    fn test_registration_preserves_page_ids() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        for page_id in 0..8 {
            fsm.register_page(page_id, 100).expect("Failed register...");
        }

        for page_id in 0..8 {
            let leaf_index = fsm.get_capacity() - 1 + page_id as usize;

            assert_eq!(fsm.nodes[leaf_index].page_id, Some(page_id));
        }
    }

    #[test]
    fn test_update_through_all_buckets() {
        let mut fsm = FreeSpaceMap::new(2).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");

        for bucket in 0..8 {
            let free_space = bucket * 512;

            fsm.update_page(0, free_space).expect("Failed update...");

            assert_eq!(fsm.nodes[1].max_bucket, Some(bucket as Bucket));

            assert_eq!(fsm.get_root().max_bucket, Some(bucket as Bucket));
        }
    }

    #[test]
    fn test_decreasing_maximum_recomputes_ancestors() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 100).expect("Failed register...");
        fsm.register_page(2, 100).expect("Failed register...");
        fsm.register_page(3, 100).expect("Failed register...");

        fsm.register_page(4, 3500).expect("Failed register...");

        assert_eq!(fsm.get_root().max_bucket, Some(6));

        fsm.update_page(4, 100).expect("Failed update...");

        assert_eq!(fsm.get_root().max_bucket, Some(0));
    }

    #[test]
    fn test_decreasing_maximum_preserves_other_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(4, 3500).expect("Failed register...");
        fsm.register_page(5, 3000).expect("Failed register...");

        assert_eq!(fsm.get_root().max_bucket, Some(6));

        fsm.update_page(4, 100).expect("Failed update...");

        assert_eq!(fsm.get_root().max_bucket, Some(5));

        assert_eq!(
            fsm.find_page(2560)
                .expect("find_page failed...")
                .expect("No page found..."),
            5
        );
    }

    #[test]
    fn test_empty_subtrees_remain_none() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");

        assert_eq!(fsm.nodes[3].max_bucket, Some(0));
        assert_eq!(fsm.nodes[4].max_bucket, None);
        assert_eq!(fsm.nodes[2].max_bucket, None);

        assert_eq!(fsm.nodes[5].max_bucket, None);
        assert_eq!(fsm.nodes[6].max_bucket, None);
    }

    #[test]
    fn test_internal_nodes_have_no_page_id() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        for page_id in 0..8 {
            fsm.register_page(page_id, 100).expect("Failed register...");
        }

        for node_index in 0..fsm.capacity - 1 {
            assert_eq!(fsm.nodes[node_index].page_id, None);
        }
    }

    #[test]
    fn test_find_page_returns_valid_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 600).expect("Failed register...");
        fsm.register_page(2, 1100).expect("Failed register...");
        fsm.register_page(3, 1600).expect("Failed register...");
        fsm.register_page(4, 2100).expect("Failed register...");
        fsm.register_page(5, 2600).expect("Failed register...");
        fsm.register_page(6, 3100).expect("Failed register...");
        fsm.register_page(7, 3600).expect("Failed register...");

        for required_bucket in 0..8 {
            let free_space = required_bucket * 512;

            let page = fsm
                .find_page(free_space)
                .expect("find_page failed...")
                .expect("No page found...");

            let leaf_index = fsm.capacity - 1 + page as usize;

            assert!(
                fsm.nodes[leaf_index].max_bucket.expect("Missing bucket")
                    >= required_bucket as Bucket
            );
        }
    }

    #[test]
    fn test_find_page_none_when_no_bucket_qualifies() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(1, 600).expect("Failed register...");
        fsm.register_page(2, 1100).expect("Failed register...");

        assert_eq!(fsm.find_page(2048).expect("find_page failed..."), None);
    }

    #[test]
    fn test_find_page_after_maximum_moves_down() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 3500).expect("Failed register...");
        fsm.register_page(4, 100).expect("Failed register...");

        assert_eq!(
            fsm.find_page(3072)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );

        fsm.update_page(0, 100).expect("Failed update...");

        assert_eq!(fsm.find_page(3072).expect("find_page failed..."), None);
    }

    #[test]
    fn test_find_page_left_subtree_priority() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 2048).expect("Failed register...");
        fsm.register_page(4, 3072).expect("Failed register...");

        let page = fsm
            .find_page(2048)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 0);
    }

    #[test]
    fn test_find_page_right_subtree_when_left_insufficient() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        fsm.register_page(4, 3072).expect("Failed register...");

        let page = fsm
            .find_page(2560)
            .expect("find_page failed...")
            .expect("No page found...");

        assert_eq!(page, 4);
    }

    #[test]
    fn test_find_page_exact_bucket_boundaries() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 512).expect("Failed register...");
        fsm.register_page(1, 1024).expect("Failed register...");
        fsm.register_page(2, 1536).expect("Failed register...");
        fsm.register_page(3, 2048).expect("Failed register...");
        fsm.register_page(4, 2560).expect("Failed register...");
        fsm.register_page(5, 3072).expect("Failed register...");
        fsm.register_page(6, 3584).expect("Failed register...");

        assert_eq!(
            fsm.find_page(512)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );

        assert_eq!(
            fsm.find_page(1024)
                .expect("find_page failed...")
                .expect("No page found..."),
            1
        );

        assert_eq!(
            fsm.find_page(1536)
                .expect("find_page failed...")
                .expect("No page found..."),
            2
        );

        assert_eq!(
            fsm.find_page(2048)
                .expect("find_page failed...")
                .expect("No page found..."),
            3
        );

        assert_eq!(
            fsm.find_page(2560)
                .expect("find_page failed...")
                .expect("No page found..."),
            4
        );

        assert_eq!(
            fsm.find_page(3072)
                .expect("find_page failed...")
                .expect("No page found..."),
            5
        );

        assert_eq!(
            fsm.find_page(3584)
                .expect("find_page failed...")
                .expect("No page found..."),
            6
        );
    }

    #[test]
    fn test_register_invalid_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        assert!(matches!(
            fsm.register_page(8, 100),
            Err(FreeSpaceMapError::InvalidPageId)
        ));
    }

    #[test]
    fn test_update_unregistered_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        assert!(matches!(
            fsm.update_page(5, 100),
            Err(FreeSpaceMapError::PageDoesNotExist)
        ));
    }

    #[test]
    fn test_duplicate_registration_updates_existing_page() {
        let mut fsm = FreeSpaceMap::new(8).expect("Unexpected behaviour...");

        fsm.register_page(0, 100).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(0));

        fsm.register_page(0, 3072).expect("Failed register...");
        assert_eq!(fsm.get_root().max_bucket, Some(6));

        assert_eq!(
            fsm.find_page(3072)
                .expect("find_page failed...")
                .expect("No page found..."),
            0
        );
    }

    #[test]
    fn test_find_page_excluding_skips_candidates() {
        use std::collections::HashSet;

        let mut fsm = FreeSpaceMap::new(8).unwrap();

        fsm.register_page(0, 600).unwrap();
        fsm.register_page(1, 1000).unwrap();
        fsm.register_page(2, 1000).unwrap();

        let excluded = HashSet::new();

        let first = fsm.find_page_excluding(700, &excluded).unwrap();

        assert_eq!(first, Some(0));

        let mut excluded = HashSet::new();
        excluded.insert(0);

        let second = fsm.find_page_excluding(700, &excluded).unwrap();

        assert_eq!(second, Some(1));

        excluded.insert(1);

        let third = fsm.find_page_excluding(700, &excluded).unwrap();

        assert_eq!(third, Some(2));

        excluded.insert(2);

        let none = fsm.find_page_excluding(700, &excluded).unwrap();

        assert_eq!(none, None);
    }
}
