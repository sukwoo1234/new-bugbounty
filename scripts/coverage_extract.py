#!/usr/bin/env python3
"""Loader-reachable covered-region extraction for V2.5 (prereg §3, §14).

Turns `llvm-cov export` output into a compact bitset over a FROZEN region universe so
step-5 set operations (asymmetric marginal |batch\\seed|, union, set-diff) are fast numpy
bitwise ops, and so we never store the 91 MB export per measurement (streaming: export →
bitset → discard export).

Region identity (frozen): (file, line_start, col_start, line_end, col_end) for CODE
regions only (llvm-cov region kind 0), DEDUPLICATED across functions (a source region
shared by multiple function instances is ONE universe region; covered if ANY instance has
execution count > 0). This dedup-kind0 definition is reproducible and independent of
inlining; it is smaller than llvm-cov's summary "regions" count (which also counts gap/
expansion regions) — we report the dedup-kind0 count as the region metric and say so.

Subcommands (export JSON read from --export FILE or stdin):
  universe --loader loader_reachable.json --out universe.json
      Enumerate all loader-reachable code regions -> stable id map (+ parse_reachable ids).
  extract --universe universe.json --out sidecar.bin
      Emit a bitset (1 bit per universe region, id order) of regions covered by this input/
      merged-profdata. Regions not in the universe (non-loader files) are ignored.

universe.json schema:
  {"region_count": N, "files": [paths...], "keys": [[file_idx,l1,c1,l2,c2]... id order],
   "parse_ids": [ids in the parse_reachable subgroup]}
"""
import json
import sys


def load_export(path):
    f = sys.stdin if (path is None or path == "-") else open(path)
    return json.load(f)


def iter_code_regions(exp):
    """Yield (file, l1, c1, l2, c2, count) for every CODE region (kind 0)."""
    for fn in exp["data"][0]["functions"]:
        names = fn["filenames"]
        for r in fn["regions"]:
            # region tuple: [l1, c1, l2, c2, count, file_id, expanded_file_id, kind]
            if r[7] != 0:
                continue
            yield names[r[5]], r[0], r[1], r[2], r[3], r[4]


def cmd_universe(args):
    loader = json.load(open(args.loader))
    full = set(loader["full_loader_files"])
    parse = set(loader["parse_reachable_files"])
    exp = load_export(args.export)

    keys = set()
    parse_keys = set()
    for f, l1, c1, l2, c2, _ in iter_code_regions(exp):
        if f not in full:
            continue
        k = (f, l1, c1, l2, c2)
        keys.add(k)
        if f in parse:
            parse_keys.add(k)

    ordered = sorted(keys)  # deterministic id assignment
    files = sorted({k[0] for k in ordered})
    file_idx = {p: i for i, p in enumerate(files)}
    key_id = {k: i for i, k in enumerate(ordered)}
    out = {
        "region_count": len(ordered),
        "files": files,
        "keys": [[file_idx[k[0]], k[1], k[2], k[3], k[4]] for k in ordered],
        "parse_ids": sorted(key_id[k] for k in parse_keys),
    }
    json.dump(out, open(args.out, "w"))
    print(f"[universe] region_count={len(ordered)} parse_reachable={len(parse_keys)} "
          f"files={len(files)} -> {args.out}")


def load_universe(path):
    u = json.load(open(path))
    files = u["files"]
    key_id = {}
    for i, (fi, l1, c1, l2, c2) in enumerate(u["keys"]):
        key_id[(files[fi], l1, c1, l2, c2)] = i
    return u["region_count"], key_id


def cmd_extract(args):
    n, key_id = load_universe(args.universe)
    exp = load_export(args.export)
    bits = bytearray((n + 7) // 8)
    covered = 0
    for f, l1, c1, l2, c2, count in iter_code_regions(exp):
        if count <= 0:
            continue
        i = key_id.get((f, l1, c1, l2, c2))
        if i is None:
            continue  # non-loader region
        byte, off = divmod(i, 8)
        if not (bits[byte] >> off) & 1:
            bits[byte] |= 1 << off
            covered += 1
    with open(args.out, "wb") as fh:
        fh.write(bits)
    print(f"[extract] covered={covered}/{n} -> {args.out}")


def main():
    import argparse
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="cmd", required=True)
    pu = sub.add_parser("universe")
    pu.add_argument("--loader", required=True)
    pu.add_argument("--export", default="-")
    pu.add_argument("--out", required=True)
    pu.set_defaults(func=cmd_universe)
    pe = sub.add_parser("extract")
    pe.add_argument("--universe", required=True)
    pe.add_argument("--export", default="-")
    pe.add_argument("--out", required=True)
    pe.set_defaults(func=cmd_extract)
    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
