#!/usr/bin/env python3
"""V2.5 RQ2 CONFIRMATORY FOLLOW-UP — within-strategy intensity sweep (byte arm).

Status: archived/deferred follow-up helper. Requires frozen coverage artifacts,
coverage_experiment.py dependencies, and the Python coverage/scientific stack.

Pre-registered as deferred work in docs/plans/coverage-freeze.json
("intensity sweep (RQ2 confirmatory)"). The frozen confirmatory run answered RQ1 and
gave only a CROSS-SEED first look at RQ2; prereg §10 RQ2 requires WITHIN-STRATEGY
variation across >=4 intensity levels so connect_rate actually moves (the cross-seed
cut is flagged in the prereg as a 2-cluster Simpson artifact).

DEVIATION (honest, construct-motivated — recorded in the sweep manifest):
  prereg §2 specifies the byte sweep as {0.5,1,2}*b_S. The frozen run measured b_S=1
  for ALL 60 seeds (structure-preserving edits change ~1 byte), so that multiplier
  ladder is DEGENERATE ({1,1,2}) and would NOT vary connect_rate -> it cannot satisfy
  the §10 requirement. This follow-up instead sweeps a per-seed FRACTIONAL byte-budget
  ladder (fraction of seed size, floored at 1 byte) so connect_rate spans its range,
  fulfilling §10's intent (>=4 levels, connect varies, within-strategy, cluster
  bootstrap). Same frozen .so / universe / seeds / harness / generation+measure path.

Reuses the frozen per-seed baselines (experiment_run/<seed>/seed.bin) and the
coverage_experiment helpers (identical generation + coverage measurement code).

Config via env: RQ2_N (outputs/level, default 50), RQ2_FRACS (csv fractions),
RQ2_WORKERS (parallel harness procs), RQ2_OUT, RQ2_FROZEN (frozen seed-baseline dir).
Usage: coverage_rq2_sweep.py <seed.onnx> [<seed.onnx> ...]
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(os.environ.get("PROJECT_ROOT", Path(__file__).resolve().parents[1]))
N = int(os.environ.get("RQ2_N", "50"))
# configure the imported orchestrator's module globals BEFORE importing it (read once).
os.environ.setdefault("COV_N_TARGET", str(N))
os.environ.setdefault("COV_RETRY_CAP", str(N * 12))
os.environ.setdefault("COV_HARNESS_WORKERS", os.environ.get("RQ2_WORKERS", "10"))
sys.path.insert(0, str(ROOT / "scripts"))
import coverage_experiment as ce  # noqa: E402

LOW = [int(x) for x in os.environ.get("RQ2_LOW", "1,4,16,64,256").split(",")]
FRACS = [float(x) for x in os.environ.get("RQ2_FRACS", "0.05,0.2,0.5").split(",")]
FROZEN = Path(os.environ.get("RQ2_FROZEN", ROOT / "data/coverage/experiment_run"))
OUT = Path(os.environ.get("RQ2_OUT", ROOT / "data/coverage/rq2_sweep"))


def budgets_for(n_bytes: int):
    """Per-seed byte-budget ladder. The smoke showed connect_rate collapses in the LOW
    budget regime (1..few-hundred bytes), so use a DENSE absolute low-end (LOW: where
    connect transitions for small/medium seeds) UNION a size-adaptive high-end
    (FRACS*size: guarantees connect~0 even for large seeds, where a 1-byte flip usually
    lands in tensor data and still loads). De-duplicated; floored at 1 byte."""
    return sorted({max(1, b) for b in LOW} | {max(1, round(f * n_bytes)) for f in FRACS})


def main():
    seeds = [Path(s) for s in sys.argv[1:]]
    if not seeds:
        print("usage: coverage_rq2_sweep.py <seed.onnx> [<seed.onnx> ...]")
        sys.exit(2)
    assert ce.SO.exists(), f"missing -O0 .so: {ce.SO}"
    assert ce.UNIVERSE.exists(), f"missing universe: {ce.UNIVERSE}"
    OUT.mkdir(parents=True, exist_ok=True)
    work = OUT / "_work"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    harness = ce.compile_harness(work)

    so_hash = ce.sh(["sha256sum", str(ce.SO.resolve())],
                    stdout=subprocess.PIPE, text=True).stdout.split()[0]
    manifest = {
        "kind": "rq2_within_strategy_sweep", "n_target": N, "low": LOW, "fracs": FRACS,
        "so": str(ce.SO), "so_sha256": so_hash, "universe": str(ce.UNIVERSE),
        "frozen_seed_dir": str(FROZEN), "workers": ce.HARNESS_WORKERS, "seeds": [],
        "deviation": ("fractional byte-budget ladder; b_S=1 (60/60 seeds) made the "
                      "prereg {0.5,1,2}*b_S sweep degenerate. Fulfills prereg §10 "
                      "within-strategy >=4-level / connect-varies requirement."),
    }

    for seed in seeds:
        sid = seed.stem
        sdir = OUT / sid
        sdir.mkdir(parents=True, exist_ok=True)
        seed_bytes = seed.read_bytes()
        buds = budgets_for(len(seed_bytes))
        gtmp = work / sid / "gen"

        # byte (havoc) arm at each intensity level
        arms = {}
        for b in buds:
            arms[f"L{b}"] = ce.gen_batch(seed, gtmp / f"L{b}", "havoc", N, budget=b)
        n_s = min((len(v) for v in arms.values()), default=0)
        for k in arms:
            arms[k] = arms[k][:n_s]

        # reuse frozen seed baseline (same .so) if available, else compute
        frozen_seed = FROZEN / sid / "seed.bin"
        if frozen_seed.exists():
            shutil.copyfile(frozen_seed, sdir / "seed.bin")
            reused = True
        else:
            ce.cover([seed], work / sid / "seedcov", harness, ce.UNIVERSE, sdir / "seed.bin")
            reused = False

        entry = {"seed": str(seed), "seed_id": sid, "n_bytes": len(seed_bytes),
                 "budgets": buds, "n_s": n_s, "seed_reused": reused, "levels": {}}
        for name, outs in arms.items():
            adir = sdir / name
            adir.mkdir(parents=True, exist_ok=True)
            files = [p for p, _ in outs]
            recs = ce.cover(files, work / sid / f"cov_{name}", harness, ce.UNIVERSE, adir / "union.bin")
            (adir / "records.json").write_text(json.dumps(
                {"arm": name, "budget": int(name[1:]), "n": len(files), "records": recs}, indent=2))
            entry["levels"][name] = {"budget": int(name[1:]), "n": len(files),
                                     "union_covered": ce.popcount(adir / "union.bin")}
        entry["seed_covered"] = ce.popcount(sdir / "seed.bin")
        manifest["seeds"].append(entry)
        print(f"[{sid}] L={len(seed_bytes)}B budgets={buds} n_s={n_s} "
              + " ".join(f"{k}={v['union_covered']}" for k, v in entry["levels"].items()),
              flush=True)

    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2))
    shutil.rmtree(work, ignore_errors=True)
    print(f"[done] rq2 sweep dataset -> {OUT}/manifest.json", flush=True)


if __name__ == "__main__":
    main()
