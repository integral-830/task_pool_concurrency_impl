use std::sync::Arc;
use std::thread;

use task_pool::storage::buffer_pool::manager::BufferPoolManager;

const THREADS: usize = 8;
const ITERATIONS: usize = 10_000;

fn temp_db_path(name: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "task_pool_{}_{}_{}.db",
        name,
        std::process::id(),
        thread_id()
    ));

    path.to_string_lossy().into_owned()
}

fn thread_id() -> u64 {
    // Stable enough to avoid collisions between concurrently-created
    // test database paths.
    let id = thread::current().id();
    let debug = format!("{id:?}");

    debug
        .trim_start_matches("ThreadId(")
        .trim_end_matches(')')
        .parse()
        .unwrap_or(0)
}

fn spawn_workers<F>(workers: usize, job: F)
where
    F: Fn(usize) + Send + Sync + 'static,
{
    let job = Arc::new(job);

    let handles: Vec<_> = (0..workers)
        .map(|worker_id| {
            let job = Arc::clone(&job);

            thread::spawn(move || {
                job(worker_id);
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("worker thread panicked");
    }
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[test]
fn concurrent_hot_page_access() {
    let path = temp_db_path("hot_pages");

    let bpm = Arc::new(BufferPoolManager::new(3, &path).expect("failed to create buffer pool"));

    // Create two pages.
    //
    // new_page() returns the page pinned, so release the pin
    // immediately before continuing.
    let page_a = bpm.new_page().expect("failed to create page A").0;

    bpm.unpin_page(page_a).expect("failed to unpin page A");

    let page_b = bpm.new_page().expect("failed to create page B").0;

    bpm.unpin_page(page_b).expect("failed to unpin page B");

    spawn_workers(THREADS, {
        let bpm = Arc::clone(&bpm);

        move |worker_id| {
            for i in 0..ITERATIONS {
                let page_id = if (worker_id + i) % 2 == 0 {
                    page_a
                } else {
                    page_b
                };

                let _frame_id = bpm.fetch_page(page_id).expect("concurrent fetch failed");

                bpm.unpin_page(page_id).expect("concurrent unpin failed");
            }
        }
    });

    let metrics = bpm.metrics();

    let expected_fetches = (THREADS * ITERATIONS) as u64;

    assert_eq!(metrics.fetches, expected_fetches);

    assert_eq!(metrics.cache_hits + metrics.cache_misses, metrics.fetches);

    assert!(metrics.cache_hits > 0);

    assert!(metrics.hit_rate >= 0.0);
    assert!(metrics.hit_rate <= 1.0);

    cleanup(&path);
}

#[test]
fn concurrent_eviction_stress() {
    let path = temp_db_path("eviction");

    let bpm = Arc::new(BufferPoolManager::new(3, &path).expect("failed to create buffer pool"));

    const PAGES: usize = 4;

    let mut pages = Vec::with_capacity(PAGES);

    // The buffer pool contains only 3 frames while we create 4 pages.
    //
    // new_page() returns a pinned page. We MUST release that pin
    // before creating the next page, otherwise CLOCK will correctly
    // report that every frame is non-evictable.
    for _ in 0..PAGES {
        let page_id = bpm.new_page().expect("failed to create page").0;

        bpm.unpin_page(page_id).expect("failed to unpin page");

        pages.push(page_id);
    }

    let pages = Arc::new(pages);

    spawn_workers(THREADS, {
        let bpm = Arc::clone(&bpm);
        let pages = Arc::clone(&pages);

        move |worker_id| {
            for i in 0..ITERATIONS {
                let index = (worker_id + i) % pages.len();
                let page_id = pages[index];

                let _frame_id = bpm
                    .fetch_page(page_id)
                    .expect("fetch failed during eviction stress");

                bpm.unpin_page(page_id)
                    .expect("unpin failed during eviction stress");
            }
        }
    });

    let metrics = bpm.metrics();

    let expected_fetches = (THREADS * ITERATIONS) as u64;

    assert_eq!(metrics.fetches, expected_fetches);

    assert_eq!(metrics.cache_hits + metrics.cache_misses, metrics.fetches);

    assert!(
        metrics.evictions > 0,
        "expected eviction pressure, got zero evictions"
    );

    assert!(metrics.hit_rate >= 0.0);
    assert!(metrics.hit_rate <= 1.0);

    cleanup(&path);
}

#[test]
fn concurrent_dirty_eviction_stress() {
    let path = temp_db_path("dirty_eviction");

    let bpm = Arc::new(BufferPoolManager::new(3, &path).expect("failed to create buffer pool"));

    const PAGES: usize = 10;

    let mut pages = Vec::with_capacity(PAGES);

    // new_page() pins the page, so immediately release the pin.
    for _ in 0..PAGES {
        let page_id = bpm.new_page().expect("failed to create page").0;

        bpm.unpin_page(page_id).expect("failed to unpin page");

        pages.push(page_id);
    }

    let pages = Arc::new(pages);

    spawn_workers(THREADS, {
        let bpm = Arc::clone(&bpm);
        let pages = Arc::clone(&pages);

        move |worker_id| {
            for i in 0..ITERATIONS {
                let index = (worker_id + i) % pages.len();
                let page_id = pages[index];

                bpm.fetch_page(page_id).expect("fetch failed");

                bpm.mark_dirty_page(page_id).expect("mark dirty failed");

                bpm.unpin_page(page_id).expect("unpin failed");
            }
        }
    });

    let metrics = bpm.metrics();

    let expected_fetches = (THREADS * ITERATIONS) as u64;

    assert_eq!(metrics.fetches, expected_fetches);

    assert_eq!(metrics.cache_hits + metrics.cache_misses, metrics.fetches);

    assert!(
        metrics.evictions > 0,
        "expected dirty eviction pressure, got zero evictions"
    );

    assert!(metrics.hit_rate >= 0.0);
    assert!(metrics.hit_rate <= 1.0);

    cleanup(&path);
}

#[test]
fn concurrent_mixed_workload() {
    let path = temp_db_path("mixed");

    let bpm = Arc::new(BufferPoolManager::new(4, &path).expect("failed to create buffer pool"));

    const PAGES: usize = 12;

    let mut pages = Vec::with_capacity(PAGES);

    // new_page() pins the page. Release each pin before creating
    // the next page so the buffer pool can evict frames when needed.
    for _ in 0..PAGES {
        let page_id = bpm.new_page().expect("failed to create page").0;

        bpm.unpin_page(page_id).expect("failed to unpin page");

        pages.push(page_id);
    }

    let pages = Arc::new(pages);

    spawn_workers(THREADS, {
        let bpm = Arc::clone(&bpm);
        let pages = Arc::clone(&pages);

        move |worker_id| {
            for i in 0..ITERATIONS {
                let page_id = pages[(worker_id.wrapping_mul(7) + i) % pages.len()];

                bpm.fetch_page(page_id)
                    .expect("mixed workload fetch failed");

                // Every third operation modifies the page.
                if i % 3 == 0 {
                    bpm.mark_dirty_page(page_id)
                        .expect("mixed workload mark_dirty failed");
                }

                bpm.unpin_page(page_id)
                    .expect("mixed workload unpin failed");

                // Occasionally force an explicit flush.
                if i % 1_000 == 0 {
                    bpm.flush_page(page_id)
                        .expect("mixed workload flush failed");
                }
            }
        }
    });

    let metrics = bpm.metrics();

    let expected_fetches = (THREADS * ITERATIONS) as u64;

    assert_eq!(metrics.fetches, expected_fetches);

    assert_eq!(metrics.cache_hits + metrics.cache_misses, metrics.fetches);

    assert!(
        metrics.evictions > 0,
        "expected mixed workload eviction pressure, got zero evictions"
    );

    assert!(metrics.hit_rate >= 0.0);
    assert!(metrics.hit_rate <= 1.0);

    cleanup(&path);
}
