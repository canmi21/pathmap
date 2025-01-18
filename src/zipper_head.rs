
use core::cell::UnsafeCell;

use crate::trie_node::*;
use crate::zipper::*;
use crate::zipper_tracking::*;
use crate::dense_byte_node::CellByteNode;

/// Used to make multiple simultaneous zippers in the same map
//
//IMPLEMENTATION NOTE: The way to think about distribution of responsibility between WriteZipper and
// ZipperHead is that WriteZipper is responsible for the trie integrity machinery.  The purpose of the
// ZipperHead is to provide an client-facing API machanism to coordinate multiple zippers in the same tree
// safely.  Therefore it is possible to have a ZipperHead that sits at an ordinary node, or even in the
// middle of a node, however creating a WriteZipper means the node at the root of the WriteZipper must be
// upgraded to a CellByteNode.
pub struct ZipperHead<'parent, 'trie, V> {
    z: UnsafeCell<OwnedOrBorrowedMut<'parent, WriteZipperCore<'trie, 'static, V>>>,
    tracker_paths: SharedTrackerPaths,
}

// `ZipperHead` can be `Send` but absolutely must not be `Sync`!
unsafe impl<V: Send + Sync + Unpin> Send for ZipperHead<'_, '_, V> {}

impl<'parent, 'trie: 'parent, V: Clone + Send + Sync + Unpin> ZipperHead<'parent, 'trie, V> {

    /// Internal method to create a new borrowed ZipperHead from a WriteZipper
    pub(crate) fn new_borrowed(parent_z: &'parent mut WriteZipperCore<'trie, 'static, V>) -> Self {
        Self {
            z: UnsafeCell::new(OwnedOrBorrowedMut::Borrowed(parent_z)),
            tracker_paths: SharedTrackerPaths::default(),
        }
    }

    /// Internal method to create a new ZipperHead by taking ownership of a WriteZipper
    pub(crate) fn new_owned(z: WriteZipperCore<'trie, 'static, V>) -> Self {
        Self {
            z: UnsafeCell::new(OwnedOrBorrowedMut::Owned(z)),
            tracker_paths: SharedTrackerPaths::default(),
        }
    }

    /// A more efficient version of [read_zipper_at_path](Self::read_zipper_at_path), where the returned
    /// zipper is constrained by the `'path` lifetime
    pub fn read_zipper_at_borrowed_path<'a, 'path>(&'a self, path: &'path[u8]) -> Result<ReadZipperTracked<'a, 'path, V>, Conflict> {
        let zipper_tracker = ZipperTracker::<TrackingRead>::new(self.tracker_paths.clone(), path)?;
        let z = self.borrow_z();
        let (root_node, root_val) = z.splitting_borrow_focus();
        Ok(ReadZipperTracked::new_with_node_and_path(root_node, path.as_ref(), Some(path.len()), root_val, zipper_tracker))
    }

    /// A more efficient version of [read_zipper_at_path_unchecked](Self::read_zipper_at_path_unchecked),
    /// where the returned zipper is constrained by the `'path` lifetime
    pub unsafe fn read_zipper_at_borrowed_path_unchecked<'a, 'path>(&'a self, path: &'path[u8]) -> ReadZipperUntracked<'a, 'path, V> {
        let z = self.borrow_z();
        let (root_node, root_val) = z.splitting_borrow_focus();
        #[cfg(debug_assertions)]
        {
            let zipper_tracker = ZipperTracker::<TrackingRead>::new(self.tracker_paths.clone(), path)
                .unwrap_or_else(|conflict| panic!("Fatal error. ReadZipper at {path:?} {conflict}"));
            ReadZipperUntracked::new_with_node_and_path(root_node, path.as_ref(), Some(path.len()), root_val, Some(zipper_tracker))
        }
        #[cfg(not(debug_assertions))]
        {
            ReadZipperUntracked::new_with_node_and_path(root_node, path.as_ref(), Some(path.len()), root_val)
        }
    }

    /// Creates a new read-only [Zipper] with the path specified from the `ZipperHead`
    pub fn read_zipper_at_path<'a, K: AsRef<[u8]>>(&'a self, path: K) -> Result<ReadZipperTracked<'a, 'static, V>, Conflict> {
        let path = path.as_ref();
        let zipper_tracker = ZipperTracker::<TrackingRead>::new(self.tracker_paths.clone(), path)?;
        let z = self.borrow_z();
        let (root_node, root_val) = z.splitting_borrow_focus();
        Ok(ReadZipperTracked::new_with_node_and_cloned_path(root_node, path.as_ref(), Some(path.len()), root_val, zipper_tracker))
    }

    /// Creates a new read-only [Zipper] with the path specified from the `ZipperHead`, where the caller
    /// guarantees that there are and there never will be any conflicts with any [WriteZipper]s at this time
    /// or any time before the returned zipper is dropped
    pub unsafe fn read_zipper_at_path_unchecked<'a, K: AsRef<[u8]>>(&'a self, path: K) -> ReadZipperUntracked<'a, 'static, V> {
        let path = path.as_ref();
        let z = self.borrow_z();
        let (root_node, root_val) = z.splitting_borrow_focus();
        #[cfg(debug_assertions)]
        {
            let zipper_tracker = ZipperTracker::<TrackingRead>::new(self.tracker_paths.clone(), path)
                .unwrap_or_else(|conflict| panic!("Fatal error. ReadZipper at {path:?} {conflict}"));
            ReadZipperUntracked::new_with_node_and_cloned_path(root_node, path.as_ref(), Some(path.len()), root_val, Some(zipper_tracker))
        }
        #[cfg(not(debug_assertions))]
        {
            ReadZipperUntracked::new_with_node_and_cloned_path(root_node, path.as_ref(), Some(path.len()), root_val)
        }
    }

    /// Creates a new [WriteZipper] with the specified path from the `ZipperHead`
    pub fn write_zipper_at_exclusive_path<'a, K: AsRef<[u8]>>(&'a self, path: K) -> Result<WriteZipperTracked<'a, 'static, V>, Conflict> {
        let path = path.as_ref();
        let zipper_tracker = ZipperTracker::<TrackingWrite>::new(self.tracker_paths.clone(), path)?;
        let z = self.borrow_z();
        let (zipper_root_node, zipper_root_val) = prepare_exclusive_write_path(z, path);
        Ok(WriteZipperTracked::new_with_node_and_path_internal(zipper_root_node, Some(zipper_root_val), &[], zipper_tracker))
    }

    /// Creates a new [WriteZipper] with the specified path from the `ZipperHead`, where the caller guarantees
    /// that no existing zippers may access the specified path at any time before the `WriteZipper` is dropped
    pub unsafe fn write_zipper_at_exclusive_path_unchecked<'a, K: AsRef<[u8]>>(&'a self, path: K) -> WriteZipperUntracked<'a, 'static, V> {
        let path = path.as_ref();
        let z = self.borrow_z();
        let (zipper_root_node, zipper_root_val) = prepare_exclusive_write_path(z, path);

        #[cfg(debug_assertions)]
        {
            let tracker = ZipperTracker::<TrackingWrite>::new(self.tracker_paths.clone(), path)
                .unwrap_or_else(|conflict| panic!("Fatal error. WriteZipper at {path:?} {conflict}"));
            WriteZipperUntracked::new_with_node_and_path_internal(zipper_root_node, Some(zipper_root_val), &[], Some(tracker))
        }
        #[cfg(not(debug_assertions))]
        {
            WriteZipperUntracked::new_with_node_and_path_internal(zipper_root_node, Some(zipper_root_val), &[])
        }
    }
}

enum OwnedOrBorrowedMut<'a, T> {
    Owned(T),
    Borrowed(&'a mut T)
}

impl<'trie, V> ZipperHead<'_, 'trie, V> {
    /// Internal method to borrow the WriteZipper within the ZipperHead
    fn borrow_z(&self) -> &mut WriteZipperCore<'trie, 'static, V> {
        let owned_or_borrowed_ref = unsafe{ &mut *self.z.get() };
        match owned_or_borrowed_ref {
            OwnedOrBorrowedMut::Owned(z) => z,
            OwnedOrBorrowedMut::Borrowed(z) => z,
        }
    }
}

impl<V> Drop for ZipperHead<'_, '_, V> {
    fn drop(&mut self) {
        let z = self.borrow_z();
        z.focus_stack.advance_if_empty_twostep(|root| root, |root| root.make_mut());
    }
}

/// Ensures that the node at the specified path exists, and is a [CellByteNode]
///
/// Discussion: This function is fairly complicated because we are only able to safely access the top
///  of the focus stack.
/// There are 4 paths through this function:
/// 1. Both the WriteZipper and target path are at the root.  This is the only case where the reference
///  to the root_val is returned.
/// 2. The target path is below the focus root, in which case we traverse to the node
/// 3. The zipper focus doesn't exist, in which case we need to create it, and then follow one of the
///  other paths.
/// 4. The target path is the zipper focus
fn prepare_exclusive_write_path<'a, V: Clone + Send + Sync + Unpin>(z: &'a mut WriteZipperCore<V>, path: &[u8]) -> (&'a mut TrieNodeODRc<V>, &'a mut Option<V>)
{
    let node_key_start = z.key.node_key_start();

    //CASE 1
    //If we have a zero-length node_key and a zero-length path, we just need to make sure the root is a
    // CellNode, and we can return
    if node_key_start == z.key.prefix_buf.len() && path.len() == 0 {
        //The only situation where a WriteZipper's node_key is zero-length is when the WZ is positioned at its root
        debug_assert_eq!(z.focus_stack.depth(), 1);
        z.focus_stack.to_root();
        let stack_root = z.focus_stack.root_mut().unwrap();
        make_cell_node(stack_root);
        let root_val = z.root_val.as_mut().unwrap();
        //UGH!  Fixing this means we need a drop impl on the ZipperHead!!
        // z.focus_stack.advance_from_root(|root| Some(root.make_mut()));
        return (stack_root, root_val)
    }

    //Otherwise we need to walk to the end of the path
    let original_path_len = z.key.prefix_buf.len();
    z.key.prefix_buf.extend(path);
    let last_path_byte = z.key.prefix_buf.pop().unwrap();

    //See below...
    let z_ptr: *mut _ = z;

    //Walk along the path to get the parent node
    let mut remaining_key = &z.key.prefix_buf[node_key_start..];
    if remaining_key.len() > 0 {
        match z.focus_stack.top_mut().unwrap().node_get_child_mut(remaining_key) {
            Some((consumed_bytes, node)) => {
                //CASE 2
                remaining_key = &remaining_key[consumed_bytes..];

                let end_node = prepare_node_at_path_end(node, remaining_key);
                let cell_node = end_node.make_mut().as_tagged_mut().into_cell_node().unwrap();
                let (exclusive_node, val) = cell_node.prepare_cf(last_path_byte);

                z.key.prefix_buf.truncate(original_path_len);

                return (exclusive_node, val)
            },
            None => {
                //CASE 3

                //SAFETY: This is another "We need Polonius" case.  We're finished with the borrow if we get here.
                let z = unsafe{ &mut *z_ptr };
                z.in_zipper_mut_static_result(
                    |node, key| {
                        let new_node = if key.len() > 0 {
                            if let Some(mut remaining) = node.take_node_at_key(key) {
                                make_cell_node(&mut remaining);
                                remaining
                            } else {
                                TrieNodeODRc::new(CellByteNode::new())
                            }
                        } else {
                            TrieNodeODRc::new(CellByteNode::new())
                        };
                        node.node_set_branch(key, new_node)
                    },
                    |_, _| true);

                //Restore and fix the zipper if we added an intermediate node
                z.key.prefix_buf.truncate(original_path_len);
                if z.key.prefix_buf.len() < original_path_len {
                    z.key.prefix_buf.push(last_path_byte);
                }
                z.mend_root();
                z.descend_to_internal();

                //Try again, now that a CellNode at the zipper root has been created
                prepare_exclusive_write_path(z, path)
            }
        }
    } else {
        //CASE 4
        z.key.prefix_buf.truncate(original_path_len);

        //If the node on top of the stack is not a cell node, we need to upgrade it
        if !z.focus_stack.top().unwrap().as_tagged().is_cell_node() {
            swap_top_node(&mut z.focus_stack, &z.key, |mut existing_node| {
                make_cell_node(&mut existing_node);
                Some(existing_node)
            });
        }
        let cell_node = z.focus_stack.top_mut().unwrap().as_tagged_mut().into_cell_node().unwrap();
        let (exclusive_node, val) = cell_node.prepare_cf(last_path_byte);
        return (exclusive_node, val)
    }
}

/// Internal function.  Upgrades the node at the end of the `key` path to a CellByteNode
fn prepare_node_at_path_end<'a, V: Clone + Send + Sync>(start_node: &'a mut TrieNodeODRc<V>, key: &[u8]) -> &'a mut TrieNodeODRc<V> {
    let (remaining_key, mut node) = node_along_path_mut(start_node, key, false);

    //If remaining_key is non-zero length, split and upgrade the intervening node
    if remaining_key.len() > 0 {
        let node_ref = node.make_mut();
        let new_parent = match node_ref.take_node_at_key(remaining_key) {
            Some(downward_node) => downward_node,
            None => TrieNodeODRc::new(CellByteNode::new())
        };
        let result = node_ref.node_set_branch(remaining_key, new_parent);
        match result {
            Ok(_) => { },
            Err(replacement_node) => { *node = replacement_node; }
        }
        let (new_remaining_key, child_node) = node_along_path_mut(node, remaining_key, false);
        debug_assert_eq!(new_remaining_key.len(), 0);
        node = child_node;
    } else {
        //Otherwise just upgrade node
        make_cell_node(node);
    }
    node
}

#[cfg(test)]
mod tests {
    use crate::trie_map::BytesTrieMap;
    use crate::zipper::*;
    use crate::tests::prefix_key;
    use std::{thread, thread::ScopedJoinHandle};

    #[test]
    fn parallel_insert_test() {
        let thread_cnt = 8;
        let elements = 1024;
        let elements_per_thread = elements / thread_cnt;

        let mut map = BytesTrieMap::<usize>::new();
        let zipper_head = map.zipper_head();

        thread::scope(|scope| {
            let elements_per_thread = elements / thread_cnt;

            //Preallocate all zippers
            let mut zippers = Vec::with_capacity(thread_cnt);
            for n in (0..thread_cnt).into_iter().rev() {
                let path = &[n as u8];
                let zipper = zipper_head.write_zipper_at_exclusive_path(path).unwrap();
                zippers.push(zipper);
            };

            let mut threads: Vec<ScopedJoinHandle<()>> = Vec::with_capacity(thread_cnt);

            //Spawn all the threads
            for n in 0..thread_cnt {
                let mut zipper = zippers.pop().unwrap();
                let thread = scope.spawn(move || {
                    for i in (n * elements_per_thread)..((n + 1) * elements_per_thread) {
                        zipper.descend_to(prefix_key(&(i as u64)));
                        assert!(zipper.set_value(i).is_none());
                        zipper.reset();
                    }
                });
                threads.push(thread);
            }

            //Wait for the threads to finish
            for thread in threads {
                thread.join().unwrap();
            }
        });
        drop(zipper_head);

        //Test that the values set by the threads are correct
        for n in 0..thread_cnt {
            for i in (n * elements_per_thread)..((n + 1) * elements_per_thread) {
                let mut path = vec![n as u8];
                path.extend(prefix_key(&(i as u64)));
                assert_eq!(map.get(path), Some(&i));
            }
        }
    }

    #[test]
    fn zipper_head1() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a ZipperHead for the whole map, and make a WriteZipper for a branch within the map
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(&[0]).unwrap();
        zipper.set_value(0);
        drop(zipper);
        drop(map_head);
        assert_eq!(map.get(&[0]), Some(&0));
    }

    #[test]
    fn zipper_head2() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a ZipperHead for the whole map, and then a zipper at the root
        //This degenerate case should be identical to making a WriteZipper from the map root
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(&[]).unwrap();
        zipper.descend_to(b"test");
        zipper.set_value(0);
        drop(zipper);
        drop(map_head);
        assert_eq!(map.get("test"), Some(&0));
    }

    #[test]
    fn zipper_head3() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a WriteZipper in a plece that will require creating multiple nodes
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(b"test").unwrap();
        zipper.descend_to(b":2");
        zipper.set_value(2);
        drop(zipper);
        drop(map_head);
        assert_eq!(map.get("test:2"), Some(&2));
    }

    #[test]
    fn zipper_head4() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a WriteZipper in a plece that will require splitting an existing path
        map.insert(b"test:3", 3);
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(b"test").unwrap();
        assert!(zipper.descend_to(b":3"));
        assert_eq!(zipper.get_value(), Some(&3));
        zipper.ascend_byte();
        zipper.descend_to_byte(b'2');
        zipper.set_value(2);
        drop(zipper);
        drop(map_head);

        assert_eq!(map.get("test:2"), Some(&2));
        assert_eq!(map.get("test:3"), Some(&3));
    }

    #[test]
    fn zipper_head5() {
        let mut map = BytesTrieMap::<isize>::new();

        //Work around a "stump" (aka a zipper root, aka a CellByteNodes that belonged to a zipper that was dropped)
        let map_head = map.zipper_head();
        let zipper = map_head.write_zipper_at_exclusive_path([3]).unwrap();
        drop(zipper);
        let mut zipper = map_head.write_zipper_at_exclusive_path([3, 193, 49]).unwrap();
        zipper.descend_to_byte(42);
        zipper.set_value(42);
        drop(zipper);
        drop(map_head);
        assert_eq!(map.get([3, 193, 49, 42]), Some(&42));
    }

    #[test]
    fn zipper_head6() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make sure that inserting a WriteZipper doesn't chop off any downstream parts of the trie
        map.insert(b"test:1", 1);
        map.insert(b"test:2", 2);
        map.insert(b"test:3", 3);
        map.insert(b"test:4", 4);
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(b"test").unwrap();
        assert!(zipper.descend_to(b":3"));
        assert_eq!(zipper.get_value(), Some(&3));
        zipper.ascend_byte();
        zipper.descend_to_byte(b'5');
        zipper.set_value(5);
        drop(zipper);
        drop(map_head);

        assert_eq!(map.get("test:1"), Some(&1));
        assert_eq!(map.get("test:2"), Some(&2));
        assert_eq!(map.get("test:3"), Some(&3));
        assert_eq!(map.get("test:4"), Some(&4));
        assert_eq!(map.get("test:5"), Some(&5));
    }

    #[test]
    fn zipper_head7() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make sure I can upgrade an ordinary ByteNode into a CellNode without losing anything
        map.insert(b"test:1", 1);
        map.insert(b"test:2", 2);
        map.insert(b"test:3", 3);
        map.insert(b"test:4", 4);
        let map_head = map.zipper_head();
        let mut zipper = map_head.write_zipper_at_exclusive_path(b"test:").unwrap();
        assert!(zipper.descend_to(b"3"));
        assert_eq!(zipper.get_value(), Some(&3));
        zipper.ascend_byte();
        zipper.descend_to_byte(b'5');
        zipper.set_value(5);
        drop(zipper);
        drop(map_head);

        assert_eq!(map.get("test:1"), Some(&1));
        assert_eq!(map.get("test:2"), Some(&2));
        assert_eq!(map.get("test:3"), Some(&3));
        assert_eq!(map.get("test:4"), Some(&4));
        assert_eq!(map.get("test:5"), Some(&5));
    }

    #[test]
    fn hierarchical_zipper_heads1() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a ZipperHead for the whole map, and two child zippers
        let map_head = map.zipper_head();
        let mut a_zipper = map_head.write_zipper_at_exclusive_path(b"a").unwrap();
        let mut b_zipper = map_head.write_zipper_at_exclusive_path(b"b").unwrap();

        //Do some interleaved work with the two zippers
        a_zipper.descend_to(b"+value");
        a_zipper.set_value(0);
        a_zipper.reset();
        b_zipper.descend_to(b"+value");
        b_zipper.set_value(1);
        b_zipper.reset();

        //Try pre-creating trie in the parent that will be visited by the child zipper
        b_zipper.descend_to(b"-children-0+metadata");
        b_zipper.set_value(-3);
        b_zipper.ascend(10);

        //Make a ZipperHead on the WriteZipper, and make two more parallel zippers
        let b_head = b_zipper.zipper_head();
        let mut b0_zipper = b_head.write_zipper_at_exclusive_path(b"0").unwrap();
        let mut b1_zipper = b_head.write_zipper_at_exclusive_path(b"1").unwrap();

        //Do some interleaved work with them
        b0_zipper.descend_to(b"+value");
        b0_zipper.set_value(4);
        b1_zipper.descend_to(b"+value");
        b1_zipper.set_value(-5);

        //Drop the child zippers, so we can move the parent again
        drop(b0_zipper);
        drop(b1_zipper);
        drop(b_head);

        //Visit some of the nodes the child zippers poked at, and fix their values with the parent
        b_zipper.descend_to(b"0+metadata");
        b_zipper.set_value(3);
        b_zipper.reset();
        b_zipper.descend_to(b"-children-1+value");
        b_zipper.set_value(5);

        //Test chopping an existing non-forking path, and inserting a new ZipperHead in there
        b_zipper.reset();
        b_zipper.descend_to(b"-children-0+meta");
        let b_head = b_zipper.zipper_head();
        let mut b0_zipper = b_head.write_zipper_at_exclusive_path([]).unwrap();
        b0_zipper.descend_to(b"bolic");
        b0_zipper.set_value(6);
        drop(b0_zipper);

        //Test making a ZipperHead when the parent WriteZipper is at a location that does not exist yet
        a_zipper.reset();
        a_zipper.descend_to(b"-children-");
        let a_head = a_zipper.zipper_head();
        let mut a0_zipper = a_head.write_zipper_at_exclusive_path("0").unwrap();
        a0_zipper.descend_to(b"+value");
        a0_zipper.set_value(7);
        drop(a0_zipper);

        //We're done.
        drop(a_head);
        drop(b_head);
        drop(a_zipper);
        drop(b_zipper);
        drop(map_head);

        // for (k, v) in map.iter() {
        //     println!("{} {v}", String::from_utf8_lossy(&k));
        // }
        assert_eq!(map.val_count(), 7);
        assert_eq!(map.get(b"a+value").unwrap(), &0);
        assert_eq!(map.get(b"a-children-0+value").unwrap(), &7);
        assert_eq!(map.get(b"b+value").unwrap(), &1);
        assert_eq!(map.get(b"b-children-0+metabolic").unwrap(), &6);
        assert_eq!(map.get(b"b-children-0+metadata").unwrap(), &3);
        assert_eq!(map.get(b"b-children-0+value").unwrap(), &4);
        assert_eq!(map.get(b"b-children-1+value").unwrap(), &5);
    }

    #[test]
    fn hierarchical_zipper_heads2() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a ZipperHead for the whole map, and two child zippers
        let map_head = map.zipper_head();
        let mut a_zipper = map_head.write_zipper_at_exclusive_path(b"a").unwrap();
        let mut b_zipper = map_head.write_zipper_at_exclusive_path(b"b").unwrap();

        //Make a separate ZipperHead on each WriteZipper
        let a_head = a_zipper.zipper_head();
        let b_head = b_zipper.zipper_head();

        //Make some WriteZippers on each head
        let mut a0_zipper = a_head.write_zipper_at_exclusive_path(b"0").unwrap();
        let mut a1_zipper = a_head.write_zipper_at_exclusive_path(b"1").unwrap();
        let mut b0_zipper = b_head.write_zipper_at_exclusive_path(b"0").unwrap();
        let mut b1_zipper = b_head.write_zipper_at_exclusive_path(b"1").unwrap();

        //Do some interleaved work with them
        a0_zipper.descend_to(b"+value");
        a0_zipper.set_value(0);
        a1_zipper.descend_to(b"+value");
        a1_zipper.set_value(1);
        b0_zipper.descend_to(b"+value");
        b0_zipper.set_value(2);
        b1_zipper.descend_to(b"+value");
        b1_zipper.set_value(3);

        //We're done
        drop(a0_zipper);
        drop(a1_zipper);
        drop(b0_zipper);
        drop(b1_zipper);
        drop(a_head);
        drop(b_head);
        drop(a_zipper);
        drop(b_zipper);
        drop(map_head);

        // for (k, v) in map.iter() {
        //     println!("{} {v}", String::from_utf8_lossy(&k));
        // }
        assert_eq!(map.val_count(), 4);
        assert_eq!(map.get(b"a0+value").unwrap(), &0);
        assert_eq!(map.get(b"a1+value").unwrap(), &1);
        assert_eq!(map.get(b"b0+value").unwrap(), &2);
        assert_eq!(map.get(b"b1+value").unwrap(), &3);
    }

    #[test]
    fn hierarchical_zipper_heads3() {
        let mut map = BytesTrieMap::<isize>::new();

        //Make a ZipperHead for the whole map, and then a zipper for a branch within the map
        let map_head = map.zipper_head();
        let mut top_zipper = map_head.write_zipper_at_exclusive_path(b"0").unwrap();
        top_zipper.descend_to(b":test:");

        //Make a sub-head at a path that doesn't exist yet
        let sub_head = top_zipper.zipper_head();
        let mut sub_zipper = sub_head.write_zipper_at_exclusive_path(b"5").unwrap();

        //Set the value at the zipper's root
        sub_zipper.set_value(5);

        //Set a value below the zipper's root
        sub_zipper.descend_to(b":next:1");
        sub_zipper.set_value(1);

        drop(sub_zipper);
        drop(sub_head);
        drop(top_zipper);
        drop(map_head);

        assert_eq!(map.get("0:test:5"), Some(&5));
        assert_eq!(map.get("0:test:5:next:1"), Some(&1));
    }
}
