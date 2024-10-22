use std::marker::PhantomData;
use std::num::NonZero;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::RwLock;

use crate::trie_map::BytesTrieMap;
use crate::write_zipper::WriteZipper;
use crate::zipper::WriteZipperUntracked;
use crate::zipper::Zipper;
use crate::zipper::ZipperIteration;

pub(crate) struct TrackingRead;
pub(crate) struct TrackingWrite;

pub(crate) trait TrackingMode {
    fn as_str() -> &'static str;
}
impl TrackingMode for TrackingRead {
    fn as_str() -> &'static str {
        "read"
    }
}
impl TrackingMode for TrackingWrite {
    fn as_str() -> &'static str {
        "write"
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
        Self::new_no_check(self.all_paths.clone(), &self.this_path[..])
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

    fn check_for_write_conflict_at(
        zipper: &WriteZipperUntracked<IsTracking>,
    ) -> Result<(), Conflict> {
        if let Some(lock) = zipper.get_value() {
            return Err(Conflict {
                with: lock.clone(),
                at: zipper.path().to_vec(),
            });
        }
        Ok(())
    }

    fn check_subtree_for_conflict<P>(
        zipper: &WriteZipperUntracked<IsTracking>,
        pred: P,
    ) -> Result<(), Conflict>
    where
        P: Fn(&IsTracking) -> bool,
    {
        let origin = zipper.path();
        let mut subtree = zipper.fork_read_zipper();
        loop {
            let next_val_in_subtree = subtree.to_next_val();
            match next_val_in_subtree {
                Some(lock) => {
                    if pred(lock) {
                        let path = subtree.path();
                        let mut v = Vec::with_capacity(origin.len() + path.len());
                        v.extend_from_slice(origin);
                        v.extend_from_slice(path);
                        return Err(Conflict {
                            with: lock.clone(),
                            at: v,
                        });
                    }
                }
                None => break,
            }
        }
        Ok(())
    }
}

/// A shared registry of every outstanding zipper
#[derive(Clone, Default)]
pub(crate) struct SharedTrackerPaths(Arc<RwLock<BytesTrieMap<IsTracking>>>);

impl SharedTrackerPaths {
    fn modify_at<F, R>(&self, path: &[u8], f: F) -> R
    where
        F: FnOnce(&mut WriteZipperUntracked<IsTracking>) -> R,
    {
        let mut guard = self.0.write().unwrap();
        let mut write_zipper = guard.write_zipper_at_path(path);
        let r = f(&mut write_zipper);
        drop(write_zipper);
        r
    }

    fn try_add_writer(&self, path: &[u8]) -> Result<(), Conflict> {
        let try_add_writer_internal = |write_zipper: &mut WriteZipperUntracked<IsTracking>| {
            if write_zipper.path_exists() {
                // check for dangling paths
                Conflict::check_subtree_for_conflict(
                    write_zipper,
                    |_| true, /* any lock causes conflict */
                )
            } else {
                write_zipper.ascend_until();

                Conflict::check_for_write_conflict_at(write_zipper)?;

                let prefix_len = write_zipper.path().len();
                let suffix = &path[prefix_len..];
                write_zipper.descend_to(suffix);
                write_zipper.set_value(IsTracking::WriteZipper);
                Ok(())
            }
        };

        self.modify_at(path, try_add_writer_internal)
    }

    fn try_add_reader(&self, path: &[u8]) -> Result<(), Conflict> {
        let try_add_reader_internal = |write_zipper: &mut WriteZipperUntracked<IsTracking>| {
            let value = write_zipper.get_value_mut();
            match value {
                Some(IsTracking::WriteZipper) => Err(Conflict::write_conflict(path)),
                Some(IsTracking::ReadZipper(cnt)) => match cnt.checked_add(1) {
                    Some(new_cnt) => {
                        *cnt = new_cnt;
                        Ok(())
                    }
                    None => Err(Conflict::read_conflict(NonZero::<u32>::MAX, path)),
                },
                None => {
                    // check for the presence of a writer below
                    Conflict::check_subtree_for_conflict(write_zipper, |lock| {
                        *lock == IsTracking::WriteZipper
                    })?;
                    // check above
                    write_zipper.ascend_until();
                    if let Some(IsTracking::WriteZipper) = write_zipper.get_value() {
                        Err(Conflict::write_conflict(write_zipper.path()))
                    } else {
                        let prefix_len = write_zipper.path().len();
                        let suffix = &path[prefix_len..];
                        write_zipper.descend_to(suffix);
                        write_zipper.set_value(IsTracking::ReadZipper(NonZero::<u32>::MIN));
                        Ok(())
                    }
                }
            }
        };

        self.modify_at(path, try_add_reader_internal)
    }

    fn add_reader_unchecked(&self, path: &[u8]) {
        let add_reader = |write_zipper: &mut WriteZipperUntracked<IsTracking>| {
            let value = write_zipper.get_value_mut();
            match value {
                Some(IsTracking::WriteZipper) => (),
                Some(IsTracking::ReadZipper(cnt)) => {
                    *cnt = unsafe { NonZero::new_unchecked(cnt.get() + 1) }
                }
                None => {
                    write_zipper.set_value(IsTracking::ReadZipper(NonZero::<u32>::MIN));
                }
            }
        };

        self.modify_at(path, add_reader)
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
        let (read_zippers, write_zippers): (Vec<_>, Vec<_>) =
            all_paths
                .iter()
                .partition(|(_, is_tracking)| match is_tracking {
                    IsTracking::ReadZipper(_) => true,
                    IsTracking::WriteZipper => false,
                });
        let _ = writeln!(
            f,
            "ZipperTracker {{ type = {:?}, path = {:?}",
            M::as_str(),
            self.this_path
        );
        let _ = writeln!(f, "\tRead Zippers:");
        for (rz, _) in read_zippers.iter() {
            let _ = writeln!(f, "\t\t{rz:?}");
        }
        let _ = writeln!(f, "\tWrite Zippers:");
        for (wz, _) in write_zippers.iter() {
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

    pub fn new_no_check(shared_paths: SharedTrackerPaths, path: &[u8]) -> Self {
        shared_paths.add_reader_unchecked(path);
        Self {
            all_paths: shared_paths,
            this_path: path.to_vec(),
            _is_tracking: PhantomData,
        }
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
}

impl<M: TrackingMode> Drop for ZipperTracker<M> {
    fn drop(&mut self) {
        self.all_paths.modify_at(&self.this_path, |write_zipper| {
            let value = write_zipper.get_value_mut();
            match value {
                Some(IsTracking::WriteZipper) => {
                    write_zipper.remove_value();
                }
                Some(IsTracking::ReadZipper(cnt)) => {
                    if *cnt == NonZero::<u32>::MIN {
                        write_zipper.remove_value();
                    } else {
                        *cnt = unsafe { NonZero::new_unchecked(cnt.get() - 1) }
                    }
                }
                _ => unreachable!(),
            };
        });
    }
}
