#!/usr/bin/env python3
"""Assemble a provenance-diverse ONNX seed corpus for the V2.5 experiment (prereg §7).

Collects .onnx from labeled sources, deduplicates by sha256 (earlier sources win, so the
curated originals take priority over bulk test models), filters by size, then stratifies by
size bucket and DETERMINISTICALLY subsamples to a target count (sorted by sha256 + stride —
no RNG, fully reproducible). Copies the chosen files into OUT and writes a manifest recording
path, provenance label, sha256, and size — the frozen seed manifest for §15.

Usage:
  coverage_build_corpus.py --out DIR --target N [--min-size B] [--max-size B] \
      label1=PATH1 label2=PATH2 ...
"""
import argparse
import hashlib
import json
import os
import shutil
from collections import defaultdict


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def collect(label, root):
    out = []
    if os.path.isfile(root) and root.endswith(".onnx"):
        out.append((label, root))
        return out
    for dp, _, fns in os.walk(root):
        if "/build/" in dp + "/":
            continue
        for fn in fns:
            if fn.endswith(".onnx"):
                out.append((label, os.path.join(dp, fn)))
    return out


def bucket(sz):
    if sz < 1024:
        return "lt1k"
    if sz < 10 * 1024:
        return "1k_10k"
    if sz < 100 * 1024:
        return "10k_100k"
    return "100k_up"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--target", type=int, default=60)
    ap.add_argument("--min-size", type=int, default=64)
    ap.add_argument("--max-size", type=int, default=2 * 1024 * 1024)
    ap.add_argument("--keep-all", default="", help="comma-sep provenance labels to include fully (priority)")
    ap.add_argument("sources", nargs="+", help="label=path ...")
    a = ap.parse_args()
    keep_all = {x for x in a.keep_all.split(",") if x}

    cand = []
    for spec in a.sources:
        label, _, path = spec.partition("=")
        cand.extend(collect(label, path))

    # dedup by sha256 (first source order wins), size filter
    seen, items = set(), []
    for label, path in cand:
        try:
            sz = os.path.getsize(path)
        except OSError:
            continue
        if sz < a.min_size or sz > a.max_size:
            continue
        h = sha256(path)
        if h in seen:
            continue
        seen.add(h)
        items.append({"label": label, "src": path, "sha256": h, "size": sz})

    # stratify by size bucket; deterministic stride subsample within each bucket
    by_b = defaultdict(list)
    for it in items:
        by_b[bucket(it["size"])].append(it)
    for b in by_b:
        by_b[b].sort(key=lambda x: x["sha256"])  # deterministic order

    chosen = []
    chosen_h = set()
    # priority: include ALL items from keep-all provenances (these are the curated/independent
    # sources; keeping them fully guards against the bulk source dominating the mix).
    for it in sorted(items, key=lambda x: x["sha256"]):
        if it["label"] in keep_all:
            chosen.append(it)
            chosen_h.add(it["sha256"])
    # then round-robin across size buckets (from the remaining, non-keep-all items) up to target
    bkeys = ["lt1k", "1k_10k", "10k_100k", "100k_up"]
    rem = {b: [x for x in by_b[b] if x["sha256"] not in chosen_h] for b in bkeys}
    idx = {b: 0 for b in bkeys}
    while len(chosen) < a.target and any(idx[b] < len(rem[b]) for b in bkeys):
        for b in bkeys:
            if idx[b] < len(rem[b]) and len(chosen) < a.target:
                chosen.append(rem[b][idx[b]])
                idx[b] += 1

    os.makedirs(a.out, exist_ok=True)
    manifest = []
    for i, it in enumerate(chosen):
        base = os.path.basename(it["src"]).replace(" ", "_")
        name = f"c{i:04}_{it['label']}_{base}"
        dst = os.path.join(a.out, name)
        shutil.copyfile(it["src"], dst)
        manifest.append({"corpus_path": dst, "provenance": it["label"],
                         "source": it["src"], "sha256": it["sha256"], "size": it["size"]})
    json.dump({"count": len(manifest), "target": a.target,
               "min_size": a.min_size, "max_size": a.max_size, "seeds": manifest},
              open(os.path.join(a.out, "corpus_manifest.json"), "w"), indent=2)

    from collections import Counter
    prov = Counter(m["provenance"] for m in manifest)
    buc = Counter(bucket(m["size"]) for m in manifest)
    print(f"corpus: {len(manifest)} seeds -> {a.out}")
    print(f"  provenance: {dict(prov)}")
    print(f"  size buckets: {dict(buc)}")
    print(f"  total candidates after dedup+size filter: {len(items)}")


if __name__ == "__main__":
    main()
