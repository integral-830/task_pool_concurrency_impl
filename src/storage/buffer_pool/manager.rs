use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, RwLock};

use crate::storage::buffer_pool::clock_replacer::FrameId;
use crate::storage::buffer_pool::disk::DiskManager;
use crate::storage::buffer_pool::frame::PageId;
use crate::storage::buffer_pool::free_frames::FreeFramesError;
use crate::storage::buffer_pool::page_table::PageTableError;
use crate::storage::buffer_pool::{frame::Frame, page_table::PageTable};
use crate::storage::PAGE_SIZE;

use super::clock_replacer::ClockReplacer;
use super::free_frames::FreeFrames;
use super::metrics::{BufferPoolMetrics, MetricsSnapshot};

pub struct BufferPoolManager {
    frames: Vec<Arc<RwLock<Frame>>>,
    page_table: PageTable,
    clock_replacer: Mutex<ClockReplacer>,
    free_frames: FreeFrames,
    disk_manager: Mutex<DiskManager>,
    next_page_id: Mutex<PageId>,
    state_lock: Mutex<()>,
    state_cv: Condvar,
    in_flight_loads: Mutex<HashMap<PageId, Arc<Condvar>>>,
    metrics: BufferPoolMetrics,
}

#[derive(Debug)]
pub enum BufferPoolError {
    LockPoisoned,
    NoFreeFrame,
    NoEvictableFrame,
    DiskError(std::io::Error),
    FrameError(std::io::Error),
    PageNotFound(PageId),
    PagePinned(PageId),
    NoPageIdAvailable,
    PageTableError(PageTableError),
    FreeFrameError(FreeFramesError),
    FrameReserved,
    FrameNotReserved,
}

impl BufferPoolManager {
    pub fn new(pool_size: usize, path: &str) -> Result<Self, BufferPoolError> {
        let frames = (0..pool_size)
            .map(|_| Arc::new(RwLock::new(Frame::new())))
            .collect();
        let disk_manager = DiskManager::new(path).map_err(BufferPoolError::DiskError)?;
        Ok(Self {
            frames,
            page_table: PageTable::new(),
            clock_replacer: Mutex::new(ClockReplacer::new()),
            free_frames: FreeFrames::initialize(pool_size)
                .map_err(BufferPoolError::FreeFrameError)?,
            disk_manager: Mutex::new(disk_manager),
            next_page_id: Mutex::new(0),
            state_lock: Mutex::new(()),
            state_cv: Condvar::new(),
            in_flight_loads: Mutex::new(HashMap::new()),
            metrics: BufferPoolMetrics::new(),
        })
    }

    pub fn fetch_page(&self, page_id: PageId) -> Result<FrameId, BufferPoolError> {
        /*
         * state_lock protects the structural decision:
         *
         * PageTable lookup
         *      +
         * in-flight load ownership
         *      +
         * frame ownership/reservation
         *
         * It is NEVER held during disk I/O.
         */
        self.metrics.record_fetch();

        let mut miss_recorded = false;

        loop {
            let guard = self
                .state_lock
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?;

            /*
             * -------------------------------------------------------------
             * 1. Page already resident
             * -------------------------------------------------------------
             */
            if let Some(frame_id) = self
                .page_table
                .get_frame_id(page_id)
                .map_err(BufferPoolError::PageTableError)?
            {
                let mut frame = self.frames[frame_id]
                    .write()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                /*
                 * The page is currently being evicted/reloaded.
                 *
                 * Do NOT return FrameReserved.
                 *
                 * Wait until the structural state changes and retry the
                 * lookup from the beginning.
                 */
                if frame.is_reserved() {
                    drop(frame);

                    /*
                     * state_cv is paired with state_lock.
                     *
                     * wait() atomically:
                     *   1. releases state_lock
                     *   2. sleeps
                     *   3. reacquires state_lock before returning
                     */
                    let _guard = self
                        .state_cv
                        .wait(guard)
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    continue;
                }

                self.metrics.record_hit();

                frame.pin().map_err(BufferPoolError::FrameError)?;

                frame
                    .mark_referenced()
                    .map_err(BufferPoolError::FrameError)?;

                drop(frame);
                drop(guard);

                return Ok(frame_id);
            }

            /*
             * We reached a real cache miss.
             *
             * Because this function can retry after waiting, only record
             * the miss once for this fetch request.
             */
            if !miss_recorded {
                self.metrics.record_miss();
                miss_recorded = true;
            }

            /*
             * -------------------------------------------------------------
             * 2. In-flight load ownership
             * -------------------------------------------------------------
             *
             * Only one thread is allowed to load a particular PageId.
             */
            let waiter = {
                let mut loads = self
                    .in_flight_loads
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                if let Some(waiter) = loads.get(&page_id) {
                    Some(Arc::clone(waiter))
                } else {
                    /*
                     * Unique loader for this PageId.
                     */
                    loads.insert(page_id, Arc::new(Condvar::new()));
                    None
                }
            };

            /*
             * Another thread is already loading this PageId.
             *
             * We cannot hold state_lock while waiting because the loader
             * needs state_lock to publish the page.
             */
            if let Some(waiter) = waiter {
                drop(guard);

                let mut loads = self
                    .in_flight_loads
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                /*
                 * Re-check because the loader could have completed between
                 * the previous lookup and acquiring this mutex.
                 */
                while loads.contains_key(&page_id) {
                    loads = waiter
                        .wait(loads)
                        .map_err(|_| BufferPoolError::LockPoisoned)?;
                }

                drop(loads);

                /*
                 * Do not recursively call fetch_page().
                 *
                 * Retry the lookup in this invocation.
                 */
                continue;
            }

            /*
             * -------------------------------------------------------------
             * 3. Free frame
             * -------------------------------------------------------------
             */
            if let Some(free_frame) = self
                .free_frames
                .get_frame()
                .map_err(BufferPoolError::FreeFrameError)?
            {
                let mut frame = match self.frames[free_frame].write() {
                    Ok(frame) => frame,
                    Err(_) => {
                        self.free_frames.add_frame(free_frame).ok();

                        let waiter = {
                            let mut loads = self
                                .in_flight_loads
                                .lock()
                                .map_err(|_| BufferPoolError::LockPoisoned)?;

                            loads.remove(&page_id)
                        };

                        if let Some(waiter) = waiter {
                            waiter.notify_all();
                        }

                        return Err(BufferPoolError::LockPoisoned);
                    }
                };

                if let Err(err) = frame.reserve() {
                    drop(frame);

                    self.free_frames
                        .add_frame(free_frame)
                        .map_err(BufferPoolError::FreeFrameError)?;

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::FrameError(err));
                }

                drop(frame);
                drop(guard);

                /*
                 * Disk I/O happens WITHOUT state_lock.
                 */
                let mut buffer = [0u8; PAGE_SIZE];

                let result = self
                    .disk_manager
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?
                    .read_page(page_id, &mut buffer);

                if let Err(err) = result {
                    /*
                     * Reacquire state_lock before publishing the rollback.
                     *
                     * This makes the Reserved -> Free transition visible
                     * through the same synchronization mechanism used by
                     * state_cv waiters.
                     */
                    let guard = self
                        .state_lock
                        .lock()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    let mut frame = self.frames[free_frame]
                        .write()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    frame.set_free().map_err(BufferPoolError::FrameError)?;

                    drop(frame);

                    self.free_frames
                        .add_frame(free_frame)
                        .map_err(BufferPoolError::FreeFrameError)?;

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    self.state_cv.notify_all();

                    drop(guard);

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::DiskError(err));
                }

                let mut frame = match self.frames[free_frame].write() {
                    Ok(frame) => frame,
                    Err(_) => {
                        /*
                         * Best-effort rollback.
                         */
                        if let Ok(mut frame) = self.frames[free_frame].write() {
                            frame.set_free().ok();
                        }

                        self.free_frames.add_frame(free_frame).ok();

                        let waiter = {
                            let mut loads = self
                                .in_flight_loads
                                .lock()
                                .map_err(|_| BufferPoolError::LockPoisoned)?;

                            loads.remove(&page_id)
                        };

                        if let Some(waiter) = waiter {
                            waiter.notify_all();
                        }

                        return Err(BufferPoolError::LockPoisoned);
                    }
                };

                if let Err(err) = frame.load_page(page_id, &buffer) {
                    frame.set_free().ok();
                    drop(frame);

                    self.free_frames
                        .add_frame(free_frame)
                        .map_err(BufferPoolError::FreeFrameError)?;

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::FrameError(err));
                }

                if let Err(err) = frame.pin() {
                    frame.set_free().ok();
                    drop(frame);

                    self.free_frames
                        .add_frame(free_frame)
                        .map_err(BufferPoolError::FreeFrameError)?;

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::FrameError(err));
                }

                if let Err(err) = frame.mark_referenced() {
                    frame.set_free().ok();
                    drop(frame);

                    self.free_frames
                        .add_frame(free_frame)
                        .map_err(BufferPoolError::FreeFrameError)?;

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::FrameError(err));
                }

                drop(frame);

                /*
                 * Publish the loaded page under state_lock.
                 */
                let guard = self
                    .state_lock
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                self.page_table
                    .insert(page_id, free_frame)
                    .map_err(BufferPoolError::PageTableError)?;

                let waiter = {
                    let mut loads = self
                        .in_flight_loads
                        .lock()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    loads.remove(&page_id)
                };

                /*
                 * The frame is no longer Reserved.
                 * Wake threads waiting because all/free frames were
                 * temporarily unavailable.
                 */
                self.state_cv.notify_all();

                drop(guard);

                if let Some(waiter) = waiter {
                    waiter.notify_all();
                }

                return Ok(free_frame);
            }

            /*
             * -------------------------------------------------------------
             * 4. CLOCK eviction
             * -------------------------------------------------------------
             */
            let victim = self
                .clock_replacer
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?
                .find_victim(&self.frames)?;

            let victim = match victim {
                Some(victim) => victim,

                None => {
                    /*
                     * CLOCK found no victim.
                     *
                     * This can mean two different things:
                     *
                     * A) Temporary:
                     *    another thread has Reserved frames and is doing
                     *    disk I/O. Wait for that operation.
                     *
                     * B) Genuine:
                     *    no frame is Reserved and every frame is pinned.
                     *    There is nothing we can evict.
                     */
                    let mut has_reserved_frame = false;

                    for frame in &self.frames {
                        let frame = frame.read().map_err(|_| BufferPoolError::LockPoisoned)?;

                        if frame.is_reserved() {
                            has_reserved_frame = true;
                            break;
                        }
                    }

                    if has_reserved_frame {
                        /*
                         * state_lock is still held here.
                         *
                         * wait() releases it while sleeping and reacquires
                         * it before returning.
                         */
                        let _guard = self
                            .state_cv
                            .wait(guard)
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        continue;
                    }

                    /*
                     * No Reserved frames exist.
                     *
                     * Therefore this is a genuine "nothing can currently
                     * be evicted" condition.
                     */
                    return Err(BufferPoolError::NoEvictableFrame);
                }
            };

            /*
             * -------------------------------------------------------------
             * 5. Reserve victim
             * -------------------------------------------------------------
             */
            let mut frame = self.frames[victim]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;

            frame.reserve().map_err(BufferPoolError::FrameError)?;

            let (old_page_id, dirty, old_data) = {
                let old_page_id = frame.get_page_id().map_err(BufferPoolError::FrameError)?;

                let dirty = frame.is_dirty();

                let mut old_data = [0u8; PAGE_SIZE];

                if dirty {
                    old_data.copy_from_slice(frame.read_data());
                }

                (old_page_id, dirty, old_data)
            };

            drop(frame);
            drop(guard);

            /*
             * -------------------------------------------------------------
             * 6. Flush dirty victim
             * -------------------------------------------------------------
             *
             * No state_lock during disk I/O.
             */
            if dirty {
                let result = self
                    .disk_manager
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?
                    .write_page(old_page_id, &old_data);

                if let Err(err) = result {
                    /*
                     * Roll the frame back from Reserved -> Resident while
                     * holding state_lock so waiting threads can safely
                     * observe the transition.
                     */
                    let guard = self
                        .state_lock
                        .lock()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    let mut frame = self.frames[victim]
                        .write()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    frame.set_resident().map_err(BufferPoolError::FrameError)?;

                    drop(frame);

                    let waiter = {
                        let mut loads = self
                            .in_flight_loads
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;

                        loads.remove(&page_id)
                    };

                    /*
                     * Reserved -> Resident is now complete.
                     */
                    self.state_cv.notify_all();

                    drop(guard);

                    if let Some(waiter) = waiter {
                        waiter.notify_all();
                    }

                    return Err(BufferPoolError::DiskError(err));
                }
            }

            /*
             * -------------------------------------------------------------
             * 7. Read requested page
             * -------------------------------------------------------------
             */
            let mut buffer = [0u8; PAGE_SIZE];

            let result = self
                .disk_manager
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?
                .read_page(page_id, &mut buffer);

            if let Err(err) = result {
                /*
                 * Again restore Reserved -> Resident under state_lock.
                 */
                let guard = self
                    .state_lock
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                let mut frame = self.frames[victim]
                    .write()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                frame.set_resident().map_err(BufferPoolError::FrameError)?;

                drop(frame);

                let waiter = {
                    let mut loads = self
                        .in_flight_loads
                        .lock()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    loads.remove(&page_id)
                };

                self.state_cv.notify_all();

                drop(guard);

                if let Some(waiter) = waiter {
                    waiter.notify_all();
                }

                return Err(BufferPoolError::DiskError(err));
            }

            /*
             * -------------------------------------------------------------
             * 8. Commit eviction
             * -------------------------------------------------------------
             */
            let guard = self
                .state_lock
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?;

            let mut frame = self.frames[victim]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;

            if let Err(err) = frame.load_page(page_id, &buffer) {
                /*
                 * load_page() failed while the frame is Reserved.
                 *
                 * Restore it to Resident so we don't leave the frame
                 * permanently stuck in Reserved state.
                 */
                frame.set_resident().map_err(BufferPoolError::FrameError)?;

                drop(frame);

                let waiter = {
                    let mut loads = self
                        .in_flight_loads
                        .lock()
                        .map_err(|_| BufferPoolError::LockPoisoned)?;

                    loads.remove(&page_id)
                };

                self.state_cv.notify_all();

                drop(guard);

                if let Some(waiter) = waiter {
                    waiter.notify_all();
                }

                return Err(BufferPoolError::FrameError(err));
            }

            frame.pin().map_err(BufferPoolError::FrameError)?;

            frame
                .mark_referenced()
                .map_err(BufferPoolError::FrameError)?;

            /*
             * Atomic structural publication:
             *
             * old_page -> victim
             * becomes
             * new_page -> victim
             */
            self.page_table
                .replace(old_page_id, page_id, victim)
                .map_err(BufferPoolError::PageTableError)?;

            self.metrics.record_eviction();

            /*
             * Loading is complete.
             */
            let waiter = {
                let mut loads = self
                    .in_flight_loads
                    .lock()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;

                loads.remove(&page_id)
            };

            /*
             * Reserved -> Resident is now committed.
             *
             * Wake:
             *   - threads fetching old_page_id
             *   - threads that found no CLOCK victim because frames
             *     were temporarily Reserved
             */
            self.state_cv.notify_all();

            drop(frame);
            drop(guard);

            if let Some(waiter) = waiter {
                waiter.notify_all();
            }

            return Ok(victim);
        }
    }
    pub fn unpin_page(&self, page_id: PageId) -> Result<(), BufferPoolError> {
        let _guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if let Some(frame_id) = self
            .page_table
            .get_frame_id(page_id)
            .map_err(BufferPoolError::PageTableError)?
        {
            let mut frame = self.frames[frame_id]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            if frame.is_reserved() {
                return Err(BufferPoolError::FrameReserved);
            }
            frame.unpin().map_err(BufferPoolError::FrameError)?;
            return Ok(());
        }
        Err(BufferPoolError::PageNotFound(page_id))
    }

    pub fn mark_dirty_page(&self, page_id: PageId) -> Result<(), BufferPoolError> {
        let _guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if let Some(frame_id) = self
            .page_table
            .get_frame_id(page_id)
            .map_err(BufferPoolError::PageTableError)?
        {
            let mut frame = self.frames[frame_id]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            if frame.is_reserved() {
                return Err(BufferPoolError::FrameReserved);
            }
            frame.mark_dirty().map_err(BufferPoolError::FrameError)?;
            return Ok(());
        }
        Err(BufferPoolError::PageNotFound(page_id))
    }

    fn flush_reserved_frame(&self, frame_id: FrameId) -> Result<(), BufferPoolError> {
        let frame = self.frames[frame_id]
            .read()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if !frame.is_reserved() {
            return Err(BufferPoolError::FrameNotReserved);
        }
        if frame.is_dirty() {
            let mut buffer_data = [0u8; PAGE_SIZE];
            let page_id = frame.get_page_id().map_err(BufferPoolError::FrameError)?;
            buffer_data.copy_from_slice(&frame.read_data());
            drop(frame);
            let result = self
                .disk_manager
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?
                .write_page(page_id, &buffer_data);
            let mut frame = self.frames[frame_id]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            if let Err(err) = result {
                frame.set_resident().map_err(BufferPoolError::FrameError)?;
                return Err(BufferPoolError::DiskError(err));
            }

            let result = frame.clear_dirty();
            if let Err(err) = result {
                frame.set_resident().map_err(BufferPoolError::FrameError)?;
                return Err(BufferPoolError::FrameError(err));
            }
        }
        Ok(())
    }

    pub fn flush_page(&self, page_id: PageId) -> Result<(), BufferPoolError> {
        let guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if let Some(frame_id) = self
            .page_table
            .get_frame_id(page_id)
            .map_err(BufferPoolError::PageTableError)?
        {
            let mut frame = self.frames[frame_id]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            if frame.is_reserved() {
                return Err(BufferPoolError::FrameReserved);
            }
            frame.reserve().map_err(BufferPoolError::FrameError)?;
            if frame.is_dirty() {
                drop(frame);
                drop(guard);
                self.flush_reserved_frame(frame_id)?;
                let mut frame = self.frames[frame_id]
                    .write()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;
                frame.set_resident().map_err(BufferPoolError::FrameError)?;
            } else {
                frame.set_resident().map_err(BufferPoolError::FrameError)?;
            }
            return Ok(());
        }
        Err(BufferPoolError::PageNotFound(page_id))
    }

    pub fn flush_all_pages(&mut self) -> Result<(), BufferPoolError> {
        let ids = self
            .page_table
            .get_resident_ids()
            .map_err(BufferPoolError::PageTableError)?;
        for i in ids {
            self.flush_page(i)?;
        }
        Ok(())
    }

    pub fn delete_page(&self, page_id: PageId) -> Result<(), BufferPoolError> {
        let guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if let Some(frame_id) = self
            .page_table
            .get_frame_id(page_id)
            .map_err(BufferPoolError::PageTableError)?
        {
            let mut frame = self.frames[frame_id]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            if !frame.is_pinned() {
                if !frame.is_reserved() {
                    frame.reserve().map_err(BufferPoolError::FrameError)?;
                    if frame.is_dirty() {
                        drop(frame);
                        drop(guard);
                        let result = self.flush_reserved_frame(frame_id);
                        if let Err(err) = result {
                            let mut frame = self.frames[frame_id]
                                .write()
                                .map_err(|_| BufferPoolError::LockPoisoned)?;
                            frame.set_resident().map_err(BufferPoolError::FrameError)?;
                            return Err(err);
                        }
                        let _guard = self
                            .state_lock
                            .lock()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;
                        let mut frame = self.frames[frame_id]
                            .write()
                            .map_err(|_| BufferPoolError::LockPoisoned)?;
                        self.page_table
                            .remove_page(page_id)
                            .map_err(BufferPoolError::PageTableError)?;
                        frame.reset().map_err(BufferPoolError::FrameError)?;
                        self.free_frames
                            .add_frame(frame_id)
                            .map_err(BufferPoolError::FreeFrameError)?;
                        return Ok(());
                    }

                    self.page_table
                        .remove_page(page_id)
                        .map_err(BufferPoolError::PageTableError)?;
                    frame.reset().map_err(BufferPoolError::FrameError)?;
                    self.free_frames
                        .add_frame(frame_id)
                        .map_err(BufferPoolError::FreeFrameError)?;
                    return Ok(());
                }
            }
            return Err(BufferPoolError::PagePinned(page_id));
        }
        Err(BufferPoolError::PageNotFound(page_id))
    }

    pub fn new_page(&self) -> Result<(PageId, FrameId), BufferPoolError> {
        let guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        let new_page_id = *self
            .next_page_id
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if new_page_id >= u16::MAX {
            return Err(BufferPoolError::NoPageIdAvailable);
        }
        if let Some(frame_id) = self
            .free_frames
            .get_frame()
            .map_err(BufferPoolError::FreeFrameError)?
        {
            let buffer = [0u8; PAGE_SIZE];
            let mut frame = self.frames[frame_id].write().map_err(|_| {
                self.free_frames.add_frame(frame_id).ok();
                BufferPoolError::LockPoisoned
            })?;
            frame.load_page(new_page_id, &buffer).map_err(|err| {
                frame.set_free().ok();
                self.free_frames.add_frame(frame_id).ok();
                BufferPoolError::FrameError(err)
            })?;
            frame.pin().map_err(|err| {
                frame.set_free().ok();
                self.free_frames.add_frame(frame_id).ok();
                BufferPoolError::FrameError(err)
            })?;
            frame.mark_referenced().map_err(|err| {
                frame.set_free().ok();
                self.free_frames.add_frame(frame_id).ok();
                BufferPoolError::FrameError(err)
            })?;
            frame.mark_dirty().map_err(|err| {
                frame.set_free().ok();
                self.free_frames.add_frame(frame_id).ok();
                BufferPoolError::FrameError(err)
            })?;
            self.page_table
                .insert(new_page_id, frame_id)
                .map_err(|err| {
                    frame.set_free().ok();
                    self.free_frames.add_frame(frame_id).ok();
                    BufferPoolError::PageTableError(err)
                })?;
            *self
                .next_page_id
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)? += 1;
            return Ok((new_page_id, frame_id));
        }
        let victim = self
            .clock_replacer
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?
            .find_victim(&self.frames)?
            .ok_or(BufferPoolError::NoEvictableFrame)?;
        let (old_page_id, dirty, old_data) = {
            let mut frame = self.frames[victim]
                .write()
                .map_err(|_| BufferPoolError::LockPoisoned)?;
            frame.reserve().map_err(BufferPoolError::FrameError)?;

            let old_page_id = frame.get_page_id().map_err(BufferPoolError::FrameError)?;

            let dirty = frame.is_dirty();

            let mut old_data = [0u8; PAGE_SIZE];

            if dirty {
                old_data.copy_from_slice(frame.read_data());
            }

            drop(frame);
            (old_page_id, dirty, old_data)
        };
        if dirty {
            drop(guard);
            let result = self
                .disk_manager
                .lock()
                .map_err(|_| BufferPoolError::LockPoisoned)?
                .write_page(old_page_id, &old_data);
            if let Err(err) = result {
                let mut frame = self.frames[victim]
                    .write()
                    .map_err(|_| BufferPoolError::LockPoisoned)?;
                frame.set_resident().map_err(BufferPoolError::FrameError)?;
                return Err(BufferPoolError::DiskError(err));
            }
        } else {
            drop(guard);
        }
        let _guard = self
            .state_lock
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        let buffer = [0u8; PAGE_SIZE];
        let mut frame = self.frames[victim]
            .write()
            .map_err(|_| BufferPoolError::LockPoisoned)?;
        if let Err(err) = frame.load_page(new_page_id, &buffer) {
            frame.set_resident().map_err(BufferPoolError::FrameError)?;
            return Err(BufferPoolError::FrameError(err));
        }
        frame.pin().map_err(BufferPoolError::FrameError)?;
        frame
            .mark_referenced()
            .map_err(BufferPoolError::FrameError)?;
        frame.mark_dirty().map_err(BufferPoolError::FrameError)?;
        self.page_table
            .replace(old_page_id, new_page_id, victim)
            .map_err(BufferPoolError::PageTableError)?;

        *self
            .next_page_id
            .lock()
            .map_err(|_| BufferPoolError::LockPoisoned)? = new_page_id + 1;
        Ok((new_page_id, victim))
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }
}
