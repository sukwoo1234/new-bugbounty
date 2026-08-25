#!/usr/bin/env python3
"""V2.5 coverage-comparison ORCHESTRATOR (step-3 runner + step-4 equal-N driver).

For each seed and each mutation strategy (arm), this:
  1. Generates exactly N_s distinct valid mutated outputs via repeated `tool mutate`
     single-mode calls (refill on operator failure; dedup by sha256; deterministic
     --seed schedule; downsample = first N_s by generation order). This is the
     prereg §6 equal-N done in the orchestrator (no run_batch_mutation patch → zero
     regression to the campaign batch path).
  2. Pins B1 (havoc) strength to S's measured median byte-edit distance b_S
     (byte-budget parity, prereg §2) and also runs a 0.5x / 2x sweep when enabled.
  3. Measures REAL loader coverage on the instrumented -O0 libonnxruntime.so:
     one fresh harness process per file (ASLR off, intra=inter=1), per-input
     (parse_ok, session_ok) recorded; the N_s output profraws are MERGED into one
     union profdata -> one llvm-cov export -> one covered-region bitset (the
     primary endpoint's per-(seed,strategy) union; per-input export is avoided).
     The bare seed is measured the same way -> seed bitset.
  4. Streams: profraws/profdata/export are deleted after the bitset is written.

Output dataset (consumed by step-5 analysis):
  OUT/<seed_id>/seed.bin
  OUT/<seed_id>/<arm>/union.bin
  OUT/<seed_id>/<arm>/records.json   # per-output parse_ok/session_ok + arm metadata
  OUT/manifest.json                  # run config + universe + binary identity

Startup subtraction is NOT applied here: per-process + seed-baseline subtraction already
cancels one-time engine-init coverage (startup ⊆ seed_set; verified by determinism). The
analysis step verifies startup ⊆ seed instead.

Config via env (see DEFAULTS). Region universe + loader set must already be built
(scripts/coverage_loader_set.py, scripts/coverage_extract.py universe).
"""
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(os.environ.get("PROJECT_ROOT", "/home/ssw/bugbounty"))
ORT = ROOT / "data/targets/onnxruntime/v1.23.2/onnxruntime-1.23.2"
SO_DIR = Path(os.environ.get("COV_SO_DIR", ORT / "build/cov-o0/RelWithDebInfo"))
SO = SO_DIR / "libonnxruntime.so"
CLANG_DIR = ROOT / "data/toolchains/clang+llvm-17.0.6-x86_64-linux-gnu-ubuntu-22.04"
CLANGXX = CLANG_DIR / "bin/clang++"
LLVM_PROFDATA = CLANG_DIR / "bin/llvm-profdata"
LLVM_COV = CLANG_DIR / "bin/llvm-cov"
TOOL = ROOT / os.environ.get("TOOL_BIN", "target/debug/tool")
HARNESS_SRC = ROOT / "harnesses/coverage-onnx/harness.cc"
EXTRACT = ROOT / "scripts/coverage_extract.py"
UNIVERSE = Path(os.environ.get("COV_UNIVERSE", ROOT / "data/coverage/universe.json"))

N_TARGET = int(os.environ.get("COV_N_TARGET", "4"))
RETRY_CAP = int(os.environ.get("COV_RETRY_CAP", str(N_TARGET * 10)))
HARNESS_TIMEOUT = int(os.environ.get("COV_HARNESS_TIMEOUT", "60"))  # per-input ORT cap (s)
HARNESS_WORKERS = int(os.environ.get("COV_HARNESS_WORKERS", "8"))   # parallel harness procs
SWEEP = os.environ.get("COV_SWEEP", "0") == "1"      # add B1@0.5x / B1@2x
WITH_B0 = os.environ.get("COV_WITH_B0", "1") == "1"  # include single-bit byte_flip baseline
OUT = Path(os.environ.get("COV_OUT", ROOT / "data/coverage/experiment"))


def sh(cmd, **kw):
    return subprocess.run(cmd, **kw)


def byte_dist(a: bytes, b: bytes) -> int:
    m = min(len(a), len(b))
    return abs(len(a) - len(b)) + sum(a[i] != b[i] for i in range(m))


def compile_harness(workdir: Path) -> Path:
    out = workdir / "onnx_cov_harness"
    cmd = [str(CLANGXX), "-std=c++17", "-O1", "-g",
           "-fprofile-instr-generate", "-fcoverage-mapping",
           f"-I{ORT}/include/onnxruntime/core/session", str(HARNESS_SRC),
           f"-L{SO_DIR}", "-lonnxruntime", f"-Wl,-rpath,{SO_DIR}", "-o", str(out)]
    sh(cmd, check=True)
    return out


# The published comparison's "structure-aware 6-operator" arm. Named here rather
# than left to the tool's default set: the default is the crash-hunting set and
# includes `aggressive`, so relying on it would silently redefine this arm and make
# the poster and paper numbers unreproducible.
STRUCTURE_AWARE = ["shape", "dtype", "name", "attribute",
                   "initializer_metadata", "graph_metadata"]

# Which single operator each byte-level arm runs, for the record written beside it.
ARM_OPERATOR = {"B0": "byte_flip", "B1_05x": "havoc", "B1_1x": "havoc", "B1_2x": "havoc"}


def gen_batch(seed: Path, tmp: Path, operator, n_target, budget=None):
    """Return list of (path, bytes). Refill + dedup + deterministic --seed.

    `operator` may be a single operator name, a list of names, or None for the
    tool's default set.
    """
    tmp.mkdir(parents=True, exist_ok=True)
    seen, outs, k = set(), [], 0
    env = dict(os.environ)
    if budget is not None:
        env["ONNX_HAVOC_BYTE_BUDGET"] = str(int(budget))
    while len(outs) < n_target and k < RETRY_CAP:
        op_path = tmp / f"m{k:05}.onnx"
        cmd = [str(TOOL), "mutate", "--target", "onnx", "--input", str(seed),
               "--out", str(op_path), "--seed", str(k)]
        for op in ([operator] if isinstance(operator, str) else (operator or [])):
            cmd += ["--operator", op]
        k += 1
        r = sh(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if r.returncode != 0 or not op_path.exists():
            continue
        b = op_path.read_bytes()
        h = hashlib.sha256(b).hexdigest()
        if h in seen:
            op_path.unlink(missing_ok=True)
            continue
        seen.add(h)
        outs.append((op_path, b))
    return outs


def cover(files, tmp: Path, harness: Path, universe: Path, bitset_out: Path):
    """Run harness per file (1 proc, ASLR off), merge profraws, export+extract -> bitset.
    Returns per-file records [(path, parse_ok, session_ok, bytes)]."""
    tmp.mkdir(parents=True, exist_ok=True)

    def run_one(i, f):
        pr = tmp / f"p{i:05}.profraw"
        env = dict(os.environ)
        env["LLVM_PROFILE_FILE"] = str(pr)
        rec = {"path": str(f), "parse_ok": None, "session_ok": None}
        try:
            # bytes (not text=): ONNX error messages may echo raw model bytes; harness
            # already \u-escapes them but decode defensively. timeout: a corrupted model
            # can genuinely hang ORT session-create — record it, don't block the run.
            r = subprocess.run(["setarch", "-R", str(harness), str(f)], env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               timeout=HARNESS_TIMEOUT)
            out = r.stdout.decode("utf-8", errors="replace")
            for line in out.splitlines():
                line = line.strip()
                if line.startswith("{"):
                    try:
                        j = json.loads(line)
                        rec.update({"path": j.get("path", str(f)),
                                    "parse_ok": j.get("parse_ok"),
                                    "session_ok": j.get("session_ok"),
                                    "bytes": j.get("bytes")})
                    except json.JSONDecodeError:
                        pass
            return rec, (pr if pr.exists() else None)
        except subprocess.TimeoutExpired:
            rec.update({"session_ok": False, "parse_ok": None, "error": "timeout"})
            pr.unlink(missing_ok=True)  # partial/absent profile on a killed process
            return rec, None

    # Parallelize per-input harness runs (each is an independent process writing its own
    # profraw; ASLR-off + single-thread ORT keeps coverage deterministic regardless of
    # concurrency). Results reassembled in input order.
    results = [None] * len(files)
    with ThreadPoolExecutor(max_workers=HARNESS_WORKERS) as ex:
        futs = {ex.submit(run_one, i, f): i for i, f in enumerate(files)}
        for fut in as_completed(futs):
            results[futs[fut]] = fut.result()
    records = [r for r, _ in results]
    profraws = [p for _, p in results if p is not None]
    if not profraws:
        # nothing produced a profile; emit an empty bitset
        bitset_out.write_bytes(b"")
        return records
    pd = tmp / "merged.profdata"
    sh([str(LLVM_PROFDATA), "merge", "-sparse", *map(str, profraws), "-o", str(pd)], check=True)
    # export | extract  (avoid 91MB temp; stream)
    with open(bitset_out, "wb") as _:
        pass
    p1 = subprocess.Popen([str(LLVM_COV), "export", str(SO), f"-instr-profile={pd}"],
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    p2 = subprocess.Popen([sys.executable, str(EXTRACT), "extract",
                           "--universe", str(universe), "--export", "-", "--out", str(bitset_out)],
                          stdin=p1.stdout, stdout=subprocess.DEVNULL)
    p1.stdout.close()
    p2.wait()
    p1.wait()
    for pr in profraws:
        pr.unlink(missing_ok=True)
    pd.unlink(missing_ok=True)
    return records


def popcount(path: Path) -> int:
    return sum(bin(byte).count("1") for byte in path.read_bytes())


def main():
    seeds = [Path(s) for s in sys.argv[1:]]
    if not seeds:
        print("usage: coverage_experiment.py <seed.onnx> [<seed.onnx> ...]")
        sys.exit(2)
    assert SO.exists(), f"missing -O0 .so: {SO}"
    assert UNIVERSE.exists(), f"missing universe: {UNIVERSE} (build with coverage_extract.py universe)"
    OUT.mkdir(parents=True, exist_ok=True)
    work = OUT / "_work"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    harness = compile_harness(work)

    so_hash = sh(["sha256sum", str(SO.resolve())], stdout=subprocess.PIPE, text=True).stdout.split()[0]
    manifest = {"n_target": N_TARGET, "so": str(SO), "so_sha256": so_hash,
                "universe": str(UNIVERSE), "seeds": [], "sweep": SWEEP, "with_b0": WITH_B0}

    for seed in seeds:
        sid = seed.stem
        sdir = OUT / sid
        sdir.mkdir(parents=True, exist_ok=True)
        seed_bytes = seed.read_bytes()
        gtmp = work / sid / "gen"

        # --- generate arms (S first to get b_S) ---
        s_out = gen_batch(seed, gtmp / "S", STRUCTURE_AWARE, N_TARGET)
        b_s = int(statistics.median([byte_dist(seed_bytes, b) for _, b in s_out])) if s_out else 1
        b_s = max(1, b_s)
        arms = {"S": s_out}

        if WITH_B0:
            arms["B0"] = gen_batch(seed, gtmp / "B0", "byte_flip", N_TARGET)
        arms["B1_1x"] = gen_batch(seed, gtmp / "B1_1x", "havoc", N_TARGET, budget=b_s)
        if SWEEP:
            arms["B1_05x"] = gen_batch(seed, gtmp / "B1_05x", "havoc", N_TARGET, budget=max(1, b_s // 2))
            arms["B1_2x"] = gen_batch(seed, gtmp / "B1_2x", "havoc", N_TARGET, budget=b_s * 2)

        # --- equal-N: N_s = min achieved across arms; downsample first-N_s ---
        n_s = min(len(v) for v in arms.values()) if arms else 0
        for k in arms:
            arms[k] = arms[k][:n_s]

        # --- seed baseline ---
        cover([seed], work / sid / "seedcov", harness, UNIVERSE, sdir / "seed.bin")

        # --- per-arm union coverage ---
        seed_entry = {"seed": str(seed), "seed_id": sid, "b_s": b_s, "n_s": n_s, "arms": {}}
        for name, outs in arms.items():
            adir = sdir / name
            adir.mkdir(parents=True, exist_ok=True)
            files = [p for p, _ in outs]
            recs = cover(files, work / sid / f"cov_{name}", harness, UNIVERSE, adir / "union.bin")
            operators = STRUCTURE_AWARE if name == "S" else [ARM_OPERATOR.get(name, "")]
            (adir / "records.json").write_text(json.dumps(
                {"arm": name, "n": len(files), "operators": operators,
                 "records": recs}, indent=2))
            seed_entry["arms"][name] = {"n": len(files),
                                        "union_covered": popcount(adir / "union.bin")}
        seed_entry["seed_covered"] = popcount(sdir / "seed.bin")
        manifest["seeds"].append(seed_entry)
        print(f"[{sid}] b_s={b_s} n_s={n_s} seed_cov={seed_entry['seed_covered']} "
              + " ".join(f"{k}={v['union_covered']}" for k, v in seed_entry["arms"].items()))

    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2))
    shutil.rmtree(work, ignore_errors=True)
    print(f"[done] dataset -> {OUT}/manifest.json")


if __name__ == "__main__":
    main()
