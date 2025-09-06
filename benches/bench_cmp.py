#!/usr/bin/env python3
import argparse, subprocess, sys
from divan_fmt import *

def main():

    parser = argparse.ArgumentParser(
        description="Compares the output of `cargo bench` from two different runs, showing difference",
        epilog="""
Example:
  python3 bench_cmp.py --base base_file --other other_file
"""
    )
    parser.add_argument('--base', type=str, help='path to the base file, from cargo bench output')
    parser.add_argument('--other', type=str, help='path to the other file, from cargo bench output')
    args = parser.parse_args()

    with open(args.base, 'r') as f:
        out = f.read()
        base_data = parse_divan_output(out)

    with open(args.other, 'r') as f:
        out = f.read()
        other_data = parse_divan_output(out)

    compared = compare_fields(base_data, other_data, 'median_ns')
    out = render_divan_table(compared)
    print(out)

if __name__ == '__main__':
    main()
