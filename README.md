
# pathmap

This crate provides a key-value store with prefix compression, structural sharing, and powerful algebraic operations.

PathMap is optimized for large data sets and can be used efficiently in a multi-threaded environment.

This crate provides the low-level data structure for [MORK](https://github.com/trueagi-io/MORK/)

## Usage

Check out the [book](https://pathmap-rs.github.io/).

## Getting Started

Until we publish version `0.1.0` on [crates.io](http://crates.io/), you'll need to pull pathmap from github by including something like this in your cargo.toml file:

```toml
pathmap = { git = "https://github.com/Adam-Vandervorst/PathMap.git" }
```

**NOTE** This is pre-alpha software and there will be API churn.  Make a fork if you need to be insulated from this churn until the initial release.

## Optional Cargo features

- `nightly`: Uses nightly-only features including support for a custom [`Allocator`](https://doc.rust-lang.org/std/alloc/trait.Allocator.html), better SIMD optimizations, etc.  Requires the *nightly* tool-chain.

- `arena_compact`: Exposes an additional read-only trie format and read-zipper type that is more efficient in memory and supports mapping a large file from disk.

- `jemalloc`: Enables [jemalloc](https://jemalloc.net/) as the default allocator.  This dramatically improves scaling for write-heavy workloads and is generally recommended.  The only reason it is not the default is to avoid interference with the host application's allocator.

- `zipper_tracking`: Exports the `zipper_tracking` module publicly, allowing the host application to use the conflict-checking logic independently of zipper creation.

- `viz`: Provide APIs to inspect and visualize pathmap trie structures.  Useful to observe structural sharing.

Other cargo features in this crate are intended for use by the developers of `pathmap` itself.
