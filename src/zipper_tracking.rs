use std::marker::PhantomData;
use std::num::NonZero;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::RwLock;

use crate::trie_map::BytesTrieMap;
use crate::write_zipper::WriteZipper;
use crate::zipper::ReadOnlyZipper;
use crate::zipper::ReadZipperUntracked;
use crate::zipper::Zipper;
use crate::zipper::ZipperAbsolutePath;
use crate::zipper::ZipperIteration;

pub(crate) struct TrackingRead;
pub(crate) struct TrackingWrite;

pub(crate) trait TrackingMode {
    fn as_str() -> &'static str;
    fn tracks_writes() -> bool;
    fn tracks_reads() -> bool {
        !Self::tracks_writes()
    }
}
impl TrackingMode for TrackingRead {
    fn as_str() -> &'static str {
        "read"
    }
    fn tracks_writes() -> bool {
        false
    }
}
impl TrackingMode for TrackingWrite {
    fn as_str() -> &'static str {
        "write"
    }
    fn tracks_writes() -> bool {
        true
    }
}

/// Tracks the root path of each zipper, to check for violations against all other outstanding zipper paths.
/// See [ZipperHead::write_zipper_at_exclusive_path_unchecked].
pub(crate) struct ZipperTracker<M: TrackingMode> {
    all_paths: SharedTrackerPaths,
    this_path: Vec<u8>,
    _is_tracking: PhantomData<M>,
}

impl Clone for ZipperTracker<TrackingRead> {
    fn clone(&self) -> Self {
        self.all_paths
            .add_reader_unchecked(self.this_path.as_slice());
        Self {
            all_paths: self.all_paths.clone(),
            this_path: self.this_path.clone(),
            _is_tracking: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Conflict {
    with: IsTracking,
    at: Vec<u8>,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let _ = write!(f, "conflicts with existing zipper: ");
        let _ = match self.with {
            IsTracking::WriteZipper => write!(f, "write zipper"),
            IsTracking::ReadZipper(cnt) => write!(f, "read zipper (arity: {cnt:?})"),
        };
        let path = &self.at;
        writeln!(f, " @ {path:?}")
    }
}

impl std::error::Error for Conflict {}

impl Conflict {
    fn write_conflict(path: &[u8]) -> Conflict {
        Conflict {
            with: IsTracking::WriteZipper,
            at: path.to_vec(),
        }
    }

    fn read_conflict(cnt: NonZeroU32, path: &[u8]) -> Conflict {
        Conflict {
            with: IsTracking::ReadZipper(cnt),
            at: path.to_vec(),
        }
    }

    fn check_for_lock_along_path<'a, A: Clone + Send + Sync + Unpin>(
        path: &[u8],
        zipper: &'a mut ReadZipperUntracked<A>,
    ) -> Option<&'a A> {
        let mut current_path = path;
        loop {
            if zipper.is_value() {
                return zipper.get_value();
            } else if current_path.is_empty() {
                return None;
            } else {
                let head = current_path[0];
                if !zipper.descend_to_byte(head) {
                    return None;
                }
                current_path = &path[1..];
            }
        }
    }

    fn check_for_write_conflict(path: &[u8], all_paths: &BytesTrieMap<()>) -> Result<(), Conflict> {
        let mut zipper = all_paths.read_zipper();
        match Conflict::check_for_lock_along_path(path, &mut zipper) {
            None =>
            /* at this point zipper is either focued on the given path (when it exists)
            , or the procedure broke out early, because it was determined that the path does not exist */
            {
                if zipper.path().len() == path.len() {
                    let mut subtree = zipper.fork_read_zipper();
                    match subtree.to_next_val() {
                        None => Ok(()),
                        Some(_) => Err(Conflict::write_conflict(
                            subtree.origin_path().unwrap_or_default(),
                        )),
                    }
                } else {
                    Ok(())
                }
            }
            Some(_) => Err(Conflict::write_conflict(zipper.path())),
        }
    }

    fn check_for_read_conflict(
        path: &[u8],
        all_paths: &BytesTrieMap<NonZeroU32>,
    ) -> Result<(), Conflict> {
        let mut zipper = all_paths.read_zipper();
        match Conflict::check_for_lock_along_path(path, &mut zipper) {
            None => {
                if zipper.path().len() == path.len() {
                    let mut subtree = zipper.fork_read_zipper();
                    match subtree.to_next_val() {
                        None => Ok(()),
                        Some(lock) => Err(Conflict::read_conflict(
                            *lock,
                            subtree.origin_path().unwrap_or_default(),
                        )),
                    }
                } else {
                    Ok(())
                }
            }
            Some(lock) => Err(Conflict::read_conflict(*lock, zipper.path())),
        }
    }
}

/// A shared registry of every outstanding zipper
#[derive(Clone, Default)]
pub(crate) struct SharedTrackerPaths(Arc<RwLock<TrackerPaths>>);

#[derive(Clone, Default)]
struct TrackerPaths {
    read_paths: BytesTrieMap<NonZeroU32>,
    written_paths: BytesTrieMap<()>,
}

impl SharedTrackerPaths {
    fn with_paths<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut TrackerPaths) -> R,
    {
        let mut guard = self.0.write().unwrap();
        let r = f(&mut guard);
        drop(guard);
        r
    }

    fn try_add_writer(&self, path: &[u8]) -> Result<(), Conflict> {
        let try_add_writer_internal = |all_paths: &mut TrackerPaths| {
            Conflict::check_for_write_conflict(path, &all_paths.written_paths)?;
            Conflict::check_for_read_conflict(path, &all_paths.read_paths)?;
            let mut writer = all_paths.written_paths.write_zipper_at_path(path);
            writer.set_value(());
            Ok(())
        };

        self.with_paths(try_add_writer_internal)
    }

    fn try_add_reader(&self, path: &[u8]) -> Result<(), Conflict> {
        let try_add_reader_internal = |all_paths: &mut TrackerPaths| {
            Conflict::check_for_write_conflict(path, &all_paths.written_paths)?;
            let mut writer = all_paths.read_paths.write_zipper_at_path(path);
            let value = writer.get_value_mut();
            match value {
                Some(cnt) => match cnt.checked_add(1) {
                    Some(new_cnt) => {
                        *cnt = new_cnt;
                        Ok(())
                    }
                    None => Err(Conflict::read_conflict(NonZero::<u32>::MAX, path)),
                },
                None => {
                    writer.set_value(NonZero::<u32>::MIN);
                    Ok(())
                }
            }
        };

        self.with_paths(try_add_reader_internal)
    }

    fn add_reader_unchecked(&self, path: &[u8]) {
        let add_reader = |paths: &mut TrackerPaths| {
            let mut writer = paths.read_paths.write_zipper_at_path(path);
            let cnt = writer.get_value_mut().unwrap();
            *cnt = unsafe { NonZero::new_unchecked(cnt.get() + 1) }
        };

        self.with_paths(add_reader)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum IsTracking {
    WriteZipper,
    ReadZipper(NonZeroU32),
}

impl<M: TrackingMode> core::fmt::Debug for ZipperTracker<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let all_paths = self.all_paths.0.read().unwrap();
        let _ = writeln!(
            f,
            "ZipperTracker {{ type = {:?}, path = {:?}",
            M::as_str(),
            self.this_path
        );
        let _ = writeln!(f, "\tRead Zippers:");
        for (rz, cnt) in all_paths.read_paths.iter() {
            let _ = writeln!(f, "\t\t{rz:?} ({cnt:?})");
        }
        let _ = writeln!(f, "\tWrite Zippers:");
        for (wz, _) in all_paths.written_paths.iter() {
            let _ = writeln!(f, "\t\t{wz:?}");
        }
        write!(f, "}}")
    }
}

impl ZipperTracker<TrackingRead> {
    pub fn new(shared_paths: SharedTrackerPaths, path: &[u8]) -> Result<Self, Conflict> {
        shared_paths.try_add_reader(path)?;
        Ok(Self {
            all_paths: shared_paths,
            this_path: path.to_vec(),
            _is_tracking: PhantomData,
        })
    }
    fn with_owned_path(shared_paths: SharedTrackerPaths, path: Vec<u8>) -> Result<Self, Conflict> {
        shared_paths.try_add_reader(&path)?;
        Ok(Self {
            all_paths: shared_paths,
            this_path: path,
            _is_tracking: PhantomData,
        })
    }
}

impl ZipperTracker<TrackingWrite> {
    pub fn new(shared_paths: SharedTrackerPaths, path: &[u8]) -> Result<Self, Conflict> {
        shared_paths.try_add_writer(path)?;
        Ok(Self {
            all_paths: shared_paths,
            this_path: path.to_vec(),
            _is_tracking: PhantomData,
        })
    }
    /// Consumes the writer tracker, and returns a new reader tracker with the same path
    pub fn into_reader(mut self) -> ZipperTracker<TrackingRead> {
        let all_paths = self.all_paths.clone();
        let this_path = core::mem::take(&mut self.this_path);
        Self::remove_lock(&all_paths, &this_path);
        core::mem::forget(self);
        ZipperTracker::<TrackingRead>::with_owned_path(all_paths, this_path).unwrap()
    }
}

impl<M: TrackingMode> ZipperTracker<M> {
    /// Internal method to remove a lock, called after it has been confirmed to be the correct thing to do
    fn remove_lock(all_paths: &SharedTrackerPaths, this_path: &[u8]) {
        let is_removed = all_paths.with_paths(|paths| {
            if M::tracks_reads() {
                let mut write_zipper = paths.read_paths.write_zipper_at_path(this_path);
                match write_zipper.get_value_mut() {
                    Some(cnt) => {
                        if *cnt == NonZero::<u32>::MIN {
                            write_zipper.remove_value();
                        } else {
                            *cnt = unsafe { NonZero::new_unchecked(cnt.get() - 1) };
                        };
                        true
                    }
                    None => false,
                }
            } else {
                let removed = paths
                    .written_paths
                    .write_zipper_at_path(this_path)
                    .remove_value();
                removed.is_some()
            }
        });
        if !is_removed {
            panic!("Lock is missing.\nContents {this_path:#?}");
        }
    }
}

impl<M: TrackingMode> Drop for ZipperTracker<M> {
    fn drop(&mut self) {
        Self::remove_lock(&self.all_paths, &self.this_path);
    }
}
