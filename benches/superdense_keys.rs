
use divan::{Divan, Bencher, black_box};
use pathmap::trie_map::BytesTrieMap;
use pathmap::zipper::*;

fn main() {
    // Run registered benchmarks.
    let divan = Divan::from_args()
        .sample_count(5000);

    divan.main();
}

#[divan::bench(args = [1000, 2000, 4000, 8000, 16000, 32000])]
fn superdense_join(bencher: Bencher, n: u64) {
    let overlap = 0.5;
    let o = ((1. - overlap) * n as f64) as u64;

    let mut vnl = BytesTrieMap::new();
    let mut vnr = BytesTrieMap::new();
    for i in 0..n { vnl.insert(prefix_key(&i), i); }
    // println!("{:?}", vnl.root);
    for i in 0..n { assert_eq!(vnl.get(prefix_key(&i)), Some(i).as_ref()); }
    for i in n..2*n { assert_eq!(vnl.get(prefix_key(&i)), None); }
    let mut c: Vec<u64> = Vec::with_capacity(n as usize);
    vnl.iter().for_each(|(k, v)| {
        assert!(*v < n);
        assert_eq!(k, prefix_key(&v));
        c.push(from_prefix_key(k.clone()));
    });
    c.sort();
    assert_eq!(c, (0..n).collect::<Vec<u64>>());
    for i in o..(n+o) { vnr.insert(prefix_key(&i), i); }

    //Benchmark the join operation
    let mut j: BytesTrieMap<u64> = BytesTrieMap::new();
    bencher.bench_local(|| {
        *black_box(&mut j) = vnl.join(black_box(&vnr));
    });
}

#[divan::bench(args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_insert(bencher: Bencher, n: u64) {

    //Benchmark the insert operation
    let out = bencher.with_inputs(|| {
        BytesTrieMap::new()
    }).bench_local_values(|mut map| {
        for i in 0..n { black_box(&mut map).insert(prefix_key(&i), i); }
        map //Return the map so we don't drop it inside the timing loop
    });
    divan::black_box_drop(out)
}

#[divan::bench(sample_size = 1, args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_drop_bench(bencher: Bencher, n: u64) {

    //Benchmark the time taken to drop the map
    bencher.with_inputs(|| {
        let mut map = BytesTrieMap::new();
        for i in 0..n { map.insert(prefix_key(&i), i); }
        map
    }).bench_local_values(|map| {
        drop(map);
    });
}

#[divan::bench(args = [1000, 2000, 4000, 8000, 16000, 32000])]
fn superdense_get(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the get operation
    bencher.bench_local(|| {
        for i in 0..n {
            assert_eq!(map.get(prefix_key(&i)), Some(&i));
        }
    });
}

#[divan::bench(args = [1000, 2000, 4000, 8000, 16000, 32000])]
fn superdense_meet(bencher: Bencher, n: u64) {
    let overlap = 0.5;
    let o = ((1. - overlap) * n as f64) as u64;

    let mut l: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { l.insert(prefix_key(&i), i); }
    let mut r: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in o..(n+o) { r.insert(prefix_key(&i), i); }

    let mut intersection: BytesTrieMap<u64> = BytesTrieMap::new();
    bencher.bench_local(|| {
        *black_box(&mut intersection) = l.meet(black_box(&r));
    });
}

/// This tests the performance of the meet op when there are already some shared nodes between the maps
#[divan::bench(args = [1000, 2000, 4000, 8000, 16000, 32000])]
fn superdense_meet_after_join(bencher: Bencher, n: u64) {

    let mut l: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..(n/2) { l.insert(prefix_key(&i), i); }
    let mut r: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in (n/2)..n { r.insert(prefix_key(&i), i); }

    let joined = l.join(&r);
    let mut intersection: BytesTrieMap<u64> = BytesTrieMap::new();
    bencher.bench_local(|| {
        *black_box(&mut intersection) = joined.meet(black_box(&l));
    });
}

/// This tests the performance of the meet op when there are already some shared nodes between the maps
#[divan::bench(args = [1000, 2000, 4000, 8000, 16000, 32000])]
fn superdense_subtract_after_join(bencher: Bencher, n: u64) {

    let mut l: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..(n/2) { l.insert(prefix_key(&i), i); }
    let mut r: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in (n/2)..n { r.insert(prefix_key(&i), i); }

    let joined = l.join(&r);
    let mut remaining: BytesTrieMap<u64> = BytesTrieMap::new();
    bencher.bench_local(|| {
        *black_box(&mut remaining) = joined.subtract(black_box(&r));
    });
    assert_eq!(remaining.val_count(), l.val_count())
}

#[divan::bench(args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_iter(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the iterator
    let mut sink = 0;
    bencher.bench_local(|| {
        map.iter().for_each(|(_key, val)| *black_box(&mut sink) = *val);
    });
}

#[divan::bench(args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_cursor(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the cursor
    let mut sink = 0;
    bencher.bench_local(|| {
        let mut cursor = map.cursor();
        while let Some((_key, val)) = cursor.next() {
            *black_box(&mut sink) = *val
        }
    });
}

#[cfg(feature = "all_dense_nodes")]
#[divan::bench(args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_all_dense_cursor(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the cursor
    let mut sink = 0;
    bencher.bench_local(|| {
        let mut cursor = map.all_dense_cursor();
        while let Some((_key, val)) = cursor.next() {
            *black_box(&mut sink) = *val
        }
    });
}

#[divan::bench(args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_zipper_cursor(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the cursor
    let mut sink = 0;
    bencher.bench_local(|| {
        let mut zipper = map.read_zipper();
        while let Some(val) = zipper.to_next_val() {
            *black_box(&mut sink) = *val
        }
    });
}

fn prefix_key(k: &u64) -> &[u8] {
    let bs = (8 - k.leading_zeros()/8) as u8;
    let kp: *const u64 = k;
    unsafe { std::slice::from_raw_parts(kp as *const _, (bs as usize).max(1)) }
}

fn from_prefix_key(k: Vec<u8>) -> u64 {
    let mut buf = [0u8; 8];
    unsafe { core::ptr::copy_nonoverlapping(k.as_ptr(), buf.as_mut_ptr(), k.len()); }
    let shift = 64usize.saturating_sub(k.len()*8);
    u64::from_le_bytes(buf) & (!0u64 >> shift)
}

#[divan::bench(sample_size = 1, args = [100, 200, 400, 800, 1600, 3200])]
fn superdense_val_count_bench(bencher: Bencher, n: u64) {

    let mut map: BytesTrieMap<u64> = BytesTrieMap::new();
    for i in 0..n { map.insert(prefix_key(&i), i); }

    //Benchmark the time taken to count the number of values in the map
    let mut sink = 0;
    bencher.bench_local(|| {
        *black_box(&mut sink) = map.val_count()
    });
    assert_eq!(sink, n as usize);
}
