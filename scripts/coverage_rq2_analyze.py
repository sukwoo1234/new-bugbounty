#!/usr/bin/env python3
"""RQ2 WITHIN-STRATEGY analysis (prereg §10) on the intensity-sweep dataset.

Status: archived/deferred follow-up helper. Requires a completed RQ2 sweep dataset
plus numpy/scipy and the frozen coverage universe.

Question: within the byte strategy, as aggressiveness varies (intensity sweep), does
the in-harness connect_rate (session_ok rate) track real marginal parse-reachable
loader coverage? The frozen run only had a CROSS-SEED cut (flagged Simpson artifact);
this is the confirmatory WITHIN-STRATEGY view.

Per seed, per intensity level:
  connect_rate = mean(session_ok over the N outputs)
  marginal     = | (level_union \\ seed) ∩ parse_reachable |   (asymmetric set diff)
Then:
  1. per-seed WITHIN-SEED Spearman(connect, marginal)  -> sign consistency + median rho
     (cleanest: seed identity fully controlled; only intensity varies)
  2. POOLED within-strategy Spearman over all (seed×level) points, with CLUSTER
     bootstrap CI (resample WHOLE seeds — row-level resampling is pseudo-replication)
  3. sweep-validity: within-seed connect_rate spread (did intensity actually move it?)

Usage: coverage_rq2_analyze.py [SWEEP_DIR] [UNIVERSE_JSON]
"""
import json
import os
import sys
from pathlib import Path

import numpy as np

ROOT = Path(os.environ.get("PROJECT_ROOT", Path(__file__).resolve().parents[1]))
EXP = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(
    os.environ.get("RQ2_OUT", ROOT / "data/coverage/rq2_sweep"))
UNIV = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(
    os.environ.get("COV_UNIVERSE", ROOT / "data/coverage/universe.json"))
BOOT = int(os.environ.get("RQ2_BOOT", "10000"))


def load_universe():
    u = json.load(open(UNIV))
    n = u["region_count"]
    pm = np.zeros(n, dtype=bool)
    pm[np.array(u["parse_ids"], dtype=np.int64)] = True
    return n, pm


def load_bits(path, n):
    raw = np.frombuffer(open(path, "rb").read(), dtype=np.uint8)
    if raw.size == 0:
        return np.zeros(n, dtype=bool)
    return np.unpackbits(raw, count=n, bitorder="little").astype(bool)


def main():
    from scipy.stats import spearmanr
    n, pm = load_universe()
    man = json.load(open(EXP / "manifest.json"))
    seeds_meta = man["seeds"]

    per_seed = {}   # sid -> [{budget, connect, marg}]
    pts = []        # (seed_index, connect, marg, budget)
    for si, s in enumerate(seeds_meta):
        sid = s["seed_id"]
        seed = load_bits(EXP / sid / "seed.bin", n)
        rows = []
        for name, lv in sorted(s["levels"].items(), key=lambda kv: kv[1]["budget"]):
            u = load_bits(EXP / sid / name / "union.bin", n)
            marg = int(((u & ~seed) & pm).sum())
            recs = json.load(open(EXP / sid / name / "records.json"))["records"]
            sess = [1 if r.get("session_ok") else 0 for r in recs]
            connect = float(np.mean(sess)) if sess else 0.0
            rows.append({"budget": lv["budget"], "connect": connect, "marg": marg})
            pts.append((si, connect, marg, lv["budget"]))
        per_seed[sid] = rows

    print(f"=== RQ2 within-strategy intensity sweep: {len(seeds_meta)} seeds, "
          f"fracs={man.get('fracs')}, N={man.get('n_target')} ===")
    print(f"=== DEVIATION (recorded): {man.get('deviation')} ===")

    # per-seed table (compact)
    print(f"\n{'seed':<40}{'budgets→connect (marg)':<10}")
    for sid, rows in per_seed.items():
        cells = " ".join(f"b{r['budget']}:{r['connect']:.2f}/{r['marg']}" for r in rows)
        print(f"  {sid:<46} {cells}")

    # (1) per-seed within-seed Spearman
    rhos = []
    for sid, rows in per_seed.items():
        c = [r["connect"] for r in rows]
        m = [r["marg"] for r in rows]
        if len(rows) >= 3 and len(set(c)) >= 2:
            rho, _ = spearmanr(c, m)
            if not np.isnan(rho):
                rhos.append(rho)
    rhos = np.array(rhos)
    print(f"\n--- (1) per-seed WITHIN-SEED Spearman(connect, marginal) ---")
    print(f"  seeds with varying connect (usable): {len(rhos)}/{len(per_seed)}")
    if rhos.size:
        neg = int((rhos < 0).sum())
        pos = int((rhos > 0).sum())
        print(f"  median within-seed rho = {np.median(rhos):.3f}")
        print(f"  sign: {neg} negative / {pos} positive / {len(rhos) - neg - pos} zero "
              f"({100 * neg / len(rhos):.0f}% negative)")
        # one-sample Wilcoxon: are within-seed rhos centered below 0?
        try:
            from scipy.stats import wilcoxon
            nz = rhos[rhos != 0]
            if nz.size:
                pneg = wilcoxon(nz, alternative="less").pvalue
                print(f"  Wilcoxon (rho<0) p = {pneg:.4g}")
        except Exception as e:
            print(f"  (wilcoxon skipped: {e})")

    # (2) pooled within-strategy Spearman + cluster bootstrap
    seed_ids = sorted({p[0] for p in pts})
    by_seed = {si: [(c, m) for (s2, c, m, _) in pts if s2 == si] for si in seed_ids}
    allc = [c for (_, c, _, _) in pts]
    allm = [m for (_, _, m, _) in pts]
    rho_pool, p_pool = spearmanr(allc, allm)
    print(f"\n--- (2) POOLED within-strategy Spearman (all seed×level points) ---")
    print(f"  rho = {rho_pool:.3f}  p = {p_pool:.4g}  "
          f"(n_points={len(pts)}, n_seeds={len(seed_ids)})")
    rng = np.random.default_rng(12345)
    boot = []
    for _ in range(BOOT):
        samp = rng.choice(seed_ids, size=len(seed_ids), replace=True)
        cc, mm = [], []
        for si in samp:
            for (c, m) in by_seed[si]:
                cc.append(c)
                mm.append(m)
        if len(set(cc)) >= 2:
            r, _ = spearmanr(cc, mm)
            if not np.isnan(r):
                boot.append(r)
    boot = np.array(boot)
    if boot.size:
        lo, hi = np.percentile(boot, [2.5, 97.5])
        verdict = "EXCLUDES 0 (significant)" if (lo > 0 or hi < 0) else "includes 0 (n.s.)"
        print(f"  cluster bootstrap 95% CI (resample whole seeds): "
              f"[{lo:.3f}, {hi:.3f}]  (B={boot.size}) -> {verdict}")

    # (3) sweep validity
    spreads = [max(r["connect"] for r in rows) - min(r["connect"] for r in rows)
               for rows in per_seed.values() if rows]
    spreads = np.array(spreads)
    print(f"\n--- (3) sweep validity: within-seed connect_rate spread (max-min) ---")
    print(f"  median spread = {np.median(spreads):.3f}; "
          f"seeds with spread>=0.10: {int((spreads >= 0.10).sum())}/{len(spreads)}")
    print("  (low spread => the intensity ladder failed to move connect for that seed)")
    print("\n=== RQ2 SWEEP DONE ===")


if __name__ == "__main__":
    main()
