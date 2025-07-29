use rand::distr::Distribution;
use rand::prelude::StdRng;
use rand::SeedableRng;
use pathmap::fuzzer::{unbiased_descend_first_policy, DescendFirstTrieValue};
use pathmap::utils::ints::gen_int_range;
use pathmap::viz::{viz_btms, DrawConfig};
use pathmap::zipper::{ZipperMoving, ZipperWriting};

fn big() {
    let r = gen_int_range(0u64, 347298389324, 4, ());
    // cutting of first bytes for example
    let mut wz = r.into_write_zipper(&[]);
    wz.descend_to(&[0, 0, 0]);
    let pm = wz.take_map().unwrap();

    let stv = DescendFirstTrieValue{ source: pm, policy: unbiased_descend_first_policy };
    let rng = StdRng::from_seed([0; 32]);
    let samples = stv.sample_iter(rng).take(1_000_000).collect::<Vec<_>>();
    std::hint::black_box(samples);
    println!("finished 1_000_000 samples")
}

fn small() {
    let r = gen_int_range(0u32, 1 << 13, 4, ());

    let stv = DescendFirstTrieValue{ source: r.clone(), policy: unbiased_descend_first_policy };
    let rng = StdRng::from_seed([0; 32]);
    let samples = stv.sample_iter(rng).take(10).collect::<Vec<_>>();
    println!("samples {:?}", samples);
    println!("https://mermaid.live/");
    let dc = DrawConfig{ ascii: false, share_values: true, hide_values: true, color_mix: false };
    unsafe { viz_btms(&[r], &dc); }
}

fn main() {
    small();
    big();
}
