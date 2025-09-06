#!/usr/bin/env python3
import argparse, subprocess, sys
from divan_fmt import *

def main():

    parser = argparse.ArgumentParser(
        description="Calls `cargo bench` multiple times and averages results.",
        epilog="""
Example:
  python3 bench_avg.py --runs 10 my_benchmark_filter

This will run `cargo bench` 10 times with the filter `my_benchmark_filter`, and print the averaged results.
"""
    )
    parser.add_argument('--runs', type=int, default=5, help='Number of times to run `cargo bench` (default: 5)')
    (args, bench_args) = parser.parse_known_args()

    results = []

    for i in range(args.runs):
        print(f"Run {i+1}/{args.runs}...")
        proc = subprocess.run(['cargo', 'bench'] + bench_args,
                               capture_output=True, text=True)
        if proc.returncode != 0:
            print(proc.stderr, file=sys.stderr)
            sys.exit(proc.returncode)
        out = proc.stdout

        # with open('/tmp/outfile', 'r') as f:
        #     out = f.read()

        # print("Raw Benchmark Output:\n", out, "\n")
        data = parse_divan_output(out)
        # print("Parsed Benchmark Data:\n", data, "\n")
        results.append(data)

    averaged = average_fields(results)
    out = render_divan_table(averaged)
    print(out)

if __name__ == '__main__':
    main()
