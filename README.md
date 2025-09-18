
# pathmap

This crate provides a key-value store with prefix compression, structural sharing, and powerful algebraic operations.

PathMap is optimized for large data sets and can be used efficiently in a multi-threaded environment.

## Getting Started

GOAT TODO!

For more details, check out the [book](GOAT! need to host somewhere)

## Optional Cargo features

- `nightly`: Uses nightly-only features including support for a custom [`Allocator`](https://doc.rust-lang.org/std/alloc/trait.Allocator.html), better SIMD optimizations, etc.  Requires the *nightly* tool-chain.

- `arena_compact`: Exposes an additional read-only trie format and read-zipper type that is more efficient in memory and supports mapping a large file from disk.

- `jemalloc`: Enables [jemalloc](https://jemalloc.net/) as the default allocator.  This dramatically improves scaling for write-heavy workloads and is generally recommended.  The only reason it is not the default is to avoid interference with the host application's allocator.

- `zipper_tracking`: Exports the `zipper_tracking` module publicly, allowing the host application to use the conflict-checking logic independently of zipper creation.

- `viz`: Provide APIs to inspect and visualize pathmap trie structures.  Useful to observe structural sharing.

Other cargo features in this crate are intended for use by the developers of `pathmap` itself.
