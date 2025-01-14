use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::mem;

/// Wraps a value `T` with an associated key `K`, deriving an ordering by that key.
/// The const generic `KEEP_SMALLEST` decides whether we store items in a max-heap (to keep the smallest N)
/// or a min-heap (to keep the largest N).
#[repr(transparent)]
#[derive(Debug, Clone)]
struct ByKey<K, V, const KEEP_SMALLEST: bool>((K, V));

// Implementing PartialEq, Eq, PartialOrd, Ord by the key alone.
impl<K: PartialEq, V, const M: bool> PartialEq for ByKey<K, V, M> {
  fn eq(&self, other: &Self) -> bool {
    self.0 .0 == other.0 .0
  }
}
impl<K: Eq, V, const M: bool> Eq for ByKey<K, V, M> {}

impl<K: Ord, V, const M: bool> PartialOrd for ByKey<K, V, M> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}
impl<K: Ord, V, const M: bool> Ord for ByKey<K, V, M> {
  fn cmp(&self, other: &Self) -> Ordering {
    let cmp = self.0 .0.cmp(&other.0 .0);
    // If KEEP_SMALLEST=true, reverse => max-heap by key => smallest N kept.
    // If KEEP_SMALLEST=false, normal => min-heap by key => largest N kept.
    if M { cmp.reverse() } else { cmp }
  }
}

/// A fixed-size heap wrapper: either keeps the top N (largest) items, or the bottom N (smallest) items,
/// depending on the const generic `KEEP_SMALLEST`.
#[derive(Debug)]
pub struct LimitedHeap<K, V, const KEEP_SMALLEST: bool> {
  capacity: usize,
  heap: BinaryHeap<ByKey<K, V, KEEP_SMALLEST>>,
}

impl<K: Ord, V, const M: bool> LimitedHeap<K, V, M> {
  /// Create a new LimitedHeap with a given capacity.
  pub fn new(max_size: usize) -> Self {
    Self {
      capacity: max_size,
      heap: BinaryHeap::new(),
    }
  }

  /// Current number of elements in the heap.
  pub fn len(&self) -> usize {
    self.heap.len()
  }

  /// Whether the heap is empty.
  pub fn is_empty(&self) -> bool {
    self.heap.is_empty()
  }

  /// View the (value, key) at the 'top' of the heap:
  /// - If `KEEP_SMALLEST=false` => top is the smallest of the "largest" N.
  /// - If `KEEP_SMALLEST=true` => top is the largest of the "smallest" N.
  pub fn peek(&self) -> Option<&(K, V)> {
    unsafe { mem::transmute(self.heap.peek()) }
  }

  /// Insert a value-key pair, discarding if it doesn't belong among the top/bottom N.
  pub fn insert(&mut self, key: K, value: V) {
    if self.heap.len() < self.capacity {
      self.heap.push(ByKey((key, value)));
      return;
    }
    if let Some(top) = self.heap.peek() {
      // For top-N largest (M=false), the heap is a min-heap => if key > top.key, replace top.
      // For bottom-N smallest (M=true), the heap is a max-heap => if key < top.key, replace top.
      let should_replace = if M { key < top.0.0 } else { key > top.0.0 };
      if should_replace {
        self.heap.pop();
        self.heap.push(ByKey((key, value)));
      }
    }
  }

  pub fn insert_lazy<F: FnOnce() -> V>(&mut self, key: K, value: F) {
    if self.heap.len() < self.capacity {
      self.heap.push(ByKey((key, value())));
      return;
    }
    if let Some(top) = self.heap.peek() {
      // For top-N largest (M=false), the heap is a min-heap => if key > top.key, replace top.
      // For bottom-N smallest (M=true), the heap is a max-heap => if key < top.key, replace top.
      let should_replace = if M { key < top.0.0 } else { key > top.0.0 };
      if should_replace {
        self.heap.pop();
        self.heap.push(ByKey((key, value())));
      }
    }
  }

  pub fn as_slice(&self) -> &[(K, V)] {
    unsafe { mem::transmute(self.heap.as_slice()) }
  }
}

#[test]
fn test_keep_top_3_largest_integers() {
  let mut top3 = LimitedHeap::<_, _, false>::new(3);
  for x in [10, 90, 45, 120, 15, 7, 200, 60] {
    top3.insert(x, x);
  }
  // The internal heap should have just 3 items: 90, 120, 200 (in some order).
  assert_eq!(top3.len(), 3);

  // The 'top' is the smallest of the largest 3, i.e. 90
  assert_eq!(top3.peek().map(|(k, _)| k), Some(&90));

  let mut items = top3.as_slice().to_vec();
  // The heap is popped in arbitrary order, but let's just check the set of keys.
  items.sort_by_key(|(k, _)| *k);
  let keys: Vec<i32> = items.iter().map(|&(_, v)| v).collect();
  assert_eq!(keys, vec![90, 120, 200]);
}

#[test]
fn test_keep_bottom_3_smallest_integers() {
  let mut bottom3 = LimitedHeap::<_, _, true>::new(3);
  for x in [10, 90, 45, 120, 15, 7, 200, 60] {
    bottom3.insert(x, x);
  }
  // Should keep [7, 10, 15].
  assert_eq!(bottom3.len(), 3);

  // 'top' is the largest of those 3, i.e. 15.
  assert_eq!(bottom3.peek().map(|(k, _)| k), Some(&15));

  let mut items = bottom3.as_slice().to_vec();
  items.sort_by_key(|(k, _)| *k);
  let keys: Vec<i32> = items.into_iter().map(|(_, v)| v).collect();
  assert_eq!(keys, vec![7, 10, 15]);
}

#[test]
fn test_insert_less_than_capacity() {
  // Capacity is 5, but only 3 items inserted.
  let mut heap = LimitedHeap::<_, _, false>::new(5);
  for x in [2, 1, 3] {
    heap.insert(x, x);
  }
  // All should be kept, since we never exceeded capacity.
  assert_eq!(heap.len(), 3);
  let mut items = heap.as_slice().to_vec();
  items.sort_by_key(|&(k, _)| k);
  let keys: Vec<i32> = items.into_iter().map(|(_, v)| v).collect();
  assert_eq!(keys, vec![1, 2, 3]);
}

#[test]
fn test_keys_and_values_differ() {
  // Suppose we keep the top 2 items by string length, but store the original strings as `value`.
  let mut heap = LimitedHeap::<&str, usize, false>::new(2);
  let words = ["hi", "hello", "rustacean", "bye"];
  for w in words {
    heap.insert(w, w.len());
  }
  // We want the top 2 by length => "rustacean" (9), "hello" (5).
  assert_eq!(heap.len(), 2);

  // The 'top' is the smaller of the two biggest lengths => 5 ("hello").
  assert_eq!(
    heap.peek().map(|(k, _)| k),
    Some(&"hello")
  );

  let mut items = heap.as_slice().to_vec();
  items.sort_by_key(|&(_, k)| k);
  // The shortest of the kept set first => (hello, 5), then (rustacean, 9).
  assert_eq!(items[0].0, "hello");
  assert_eq!(items[1].0, "rustacean");
}
