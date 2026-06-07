#!/usr/bin/env python3
import argparse
import csv
import hashlib
import json
import os
import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


@dataclass
class RunRecord:
    run_name: str
    run_id: str
    source: str
    path: Path
    timestamp: datetime | None
    status: dict[str, Any]
    fuzzer_stats: dict[str, str]
    crash_files: int
    hang_files: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export an Arm C AFL++ summary from live run dirs and retention archives."
    )
    parser.add_argument("--experiment-id", required=True)
    parser.add_argument("--machine-label", default="06-211-01")
    parser.add_argument("--target", default="onnx")
    parser.add_argument("--backend", default="aflpp")
    parser.add_argument("--runs-root", default="data/runs")
    parser.add_argument(
        "--retention-root", default="data/experiments/aflpp-arm-c-retention"
    )
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--campaign-start", default="")
    parser.add_argument("--campaign-end", default="")
    parser.add_argument("--duration-hours", type=float, default=168.0)
    parser.add_argument("--corpus-dir", default="seeds/onnx")
    parser.add_argument("--workers", type=int, default=1)
    parser.add_argument("--timeout-sec", type=int, default=30)
    parser.add_argument("--restart-limit", type=int, default=1)
    parser.add_argument("--notes", default="")
    parser.add_argument("--include-journal", action="store_true")
    parser.add_argument(
        "--journal-unit", default="tool-fuzz-onnx-aflpp", help="systemd unit name"
    )
    return parser.parse_args()


def parse_datetime(value: str) -> datetime | None:
    if not value:
        return None
    normalized = value.strip().replace(" KST", "+09:00").replace("KST", "+09:00")
    normalized = normalized.replace(" ", "T", 1)
    parsed = datetime.fromisoformat(normalized)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.now().astimezone().tzinfo)
    return parsed.astimezone(timezone.utc)


def timestamp_from_run_id(run_id: str, fallback_path: Path) -> datetime | None:
    try:
        numeric_id = int(run_id)
        if numeric_id >= 1_000_000_000_000:
            seconds = numeric_id / 1000.0
        else:
            seconds = float(numeric_id)
        return datetime.fromtimestamp(seconds, tz=timezone.utc)
    except (OSError, OverflowError, ValueError):
        try:
            return datetime.fromtimestamp(fallback_path.stat().st_mtime, tz=timezone.utc)
        except OSError:
            return None


def load_json(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def load_fuzzer_stats(run_dir: Path) -> dict[str, str]:
    stats_files = sorted(run_dir.glob("**/fuzzer_stats"))
    if not stats_files:
        return {}
    stats: dict[str, str] = {}
    try:
        for line in stats_files[0].read_text(encoding="utf-8", errors="replace").splitlines():
            if ":" not in line:
                continue
            key, value = line.split(":", 1)
            stats[key.strip()] = value.strip()
    except OSError:
        return {}
    return stats


def count_evidence_files(run_dir: Path, evidence_name: str) -> int:
    count = 0
    for evidence_dir in run_dir.glob(f"**/{evidence_name}"):
        if not evidence_dir.is_dir():
            continue
        try:
            count += sum(1 for child in evidence_dir.iterdir() if child.is_file())
        except OSError:
            continue
    return count


def collect_records(root: Path, source: str) -> dict[str, RunRecord]:
    records: dict[str, RunRecord] = {}
    if not root.exists():
        return records
    for run_dir in sorted(root.glob("run-*")):
        if not run_dir.is_dir():
            continue
        status_path = run_dir / "status.json"
        if not status_path.is_file():
            continue
        run_name = run_dir.name
        status = load_json(status_path)
        run_id = str(status.get("run_id") or run_name.removeprefix("run-"))
        records[run_name] = RunRecord(
            run_name=run_name,
            run_id=run_id,
            source=source,
            path=run_dir,
            timestamp=timestamp_from_run_id(run_id, run_dir),
            status=status,
            fuzzer_stats=load_fuzzer_stats(run_dir),
            crash_files=count_evidence_files(run_dir, "crashes"),
            hang_files=count_evidence_files(run_dir, "hangs"),
        )
    return records


def filter_records(
    records: list[RunRecord],
    campaign_start: datetime | None,
    campaign_end: datetime | None,
) -> list[RunRecord]:
    filtered: list[RunRecord] = []
    for record in records:
        if campaign_start and record.timestamp and record.timestamp < campaign_start:
            continue
        if campaign_end and record.timestamp and record.timestamp > campaign_end:
            continue
        filtered.append(record)
    return filtered


def sum_int(records: list[RunRecord], key: str) -> int:
    total = 0
    for record in records:
        try:
            total += int(record.status.get(key, 0) or 0)
        except (TypeError, ValueError):
            continue
    return total


def sum_stat(records: list[RunRecord], key: str) -> int:
    total = 0
    for record in records:
        try:
            total += int(record.fuzzer_stats.get(key, "0") or 0)
        except ValueError:
            continue
    return total


def max_stat(records: list[RunRecord], key: str) -> int:
    values: list[int] = []
    for record in records:
        try:
            values.append(int(record.fuzzer_stats.get(key, "0") or 0))
        except ValueError:
            continue
    return max(values) if values else 0


def iso_or_empty(value: datetime | None) -> str:
    return value.isoformat().replace("+00:00", "Z") if value else ""


def git_commit() -> str:
    try:
        output = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            text=True,
            stderr=subprocess.DEVNULL,
        )
        return output.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def seed_count(corpus_dir: Path, target: str) -> int:
    if not corpus_dir.is_dir():
        return 0
    return sum(1 for child in corpus_dir.iterdir() if child.is_file() and child.suffix == f".{target}")


def write_tsv(path: Path, records: list[RunRecord]) -> None:
    with path.open("w", encoding="utf-8", newline="") as output:
        writer = csv.writer(output, delimiter="\t")
        writer.writerow(
            [
                "run_name",
                "run_id",
                "source",
                "timestamp_utc",
                "total",
                "success",
                "failed",
                "timeout",
                "retries",
                "execs_done",
                "corpus_count",
                "saved_crashes",
                "saved_hangs",
                "crash_files",
                "hang_files",
                "path",
            ]
        )
        for record in records:
            writer.writerow(
                [
                    record.run_name,
                    record.run_id,
                    record.source,
                    iso_or_empty(record.timestamp),
                    record.status.get("total", 0),
                    record.status.get("success", 0),
                    record.status.get("failed", 0),
                    record.status.get("timeout", 0),
                    record.status.get("retries", 0),
                    record.fuzzer_stats.get("execs_done", ""),
                    record.fuzzer_stats.get("corpus_count", ""),
                    record.fuzzer_stats.get("saved_crashes", ""),
                    record.fuzzer_stats.get("saved_hangs", ""),
                    record.crash_files,
                    record.hang_files,
                    record.path,
                ]
            )


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as input_file:
        for chunk in iter(lambda: input_file.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def write_file_index(out_dir: Path, relative_paths: list[str]) -> Path:
    index_path = out_dir / "file-index.tsv"
    with index_path.open("w", encoding="utf-8", newline="") as output:
        writer = csv.writer(output, delimiter="\t")
        writer.writerow(["relative_path", "size_bytes", "sha256"])
        for relative_path in relative_paths:
            artifact_path = out_dir / relative_path
            if not artifact_path.is_file():
                continue
            metadata = artifact_path.stat()
            writer.writerow([relative_path, metadata.st_size, sha256_file(artifact_path)])
    return index_path


def write_journal(args: argparse.Namespace, out_dir: Path) -> str:
    if not args.include_journal:
        return ""
    journal_path = out_dir / "journal.txt"
    command = ["journalctl", "-u", args.journal_unit, "--no-pager"]
    if args.campaign_start:
        command.extend(["--since", args.campaign_start])
    if args.campaign_end:
        command.extend(["--until", args.campaign_end])
    try:
        with journal_path.open("w", encoding="utf-8") as output:
            subprocess.run(command, check=False, stdout=output, stderr=subprocess.STDOUT)
        return str(journal_path)
    except OSError:
        return ""


def main() -> int:
    args = parse_args()
    campaign_start = parse_datetime(args.campaign_start)
    campaign_end = parse_datetime(args.campaign_end)
    out_dir = Path(args.out_dir or f"results/experiments/{args.experiment_id}")
    out_dir.mkdir(parents=True, exist_ok=True)

    live_records = collect_records(Path(args.runs_root), "live")
    archive_records = collect_records(Path(args.retention_root), "retention")
    combined = {**archive_records, **live_records}
    records = sorted(
        filter_records(list(combined.values()), campaign_start, campaign_end),
        key=lambda record: (record.timestamp or datetime.min.replace(tzinfo=timezone.utc), record.run_name),
    )

    first_timestamp = records[0].timestamp if records else None
    last_timestamp = records[-1].timestamp if records else None
    live_count = sum(1 for record in records if record.source == "live")
    archive_count = sum(1 for record in records if record.source == "retention")
    stats_count = sum(1 for record in records if record.fuzzer_stats)
    total_crash_files = sum(record.crash_files for record in records)
    total_hang_files = sum(record.hang_files for record in records)
    manifest = {
        "experiment_id": args.experiment_id,
        "machine_label": args.machine_label,
        "target": args.target,
        "backend": args.backend,
        "duration_hours": args.duration_hours,
        "campaign_start": args.campaign_start,
        "campaign_end": args.campaign_end,
        "first_run_timestamp_utc": iso_or_empty(first_timestamp),
        "last_run_timestamp_utc": iso_or_empty(last_timestamp),
        "runs_root": args.runs_root,
        "retention_root": args.retention_root,
        "record_count": len(records),
        "live_record_count": live_count,
        "retention_record_count": archive_count,
        "workers": args.workers,
        "timeout_sec": args.timeout_sec,
        "restart_limit": args.restart_limit,
        "corpus_dir": args.corpus_dir,
        "seed_count": seed_count(Path(args.corpus_dir), args.target),
        "git_commit": git_commit(),
        "total": sum_int(records, "total"),
        "success": sum_int(records, "success"),
        "failed": sum_int(records, "failed"),
        "timeout": sum_int(records, "timeout"),
        "retries": sum_int(records, "retries"),
        "fuzzer_stats_count": stats_count,
        "execs_done_sum": sum_stat(records, "execs_done"),
        "corpus_count_max": max_stat(records, "corpus_count"),
        "saved_crashes_sum": sum_stat(records, "saved_crashes"),
        "saved_hangs_sum": sum_stat(records, "saved_hangs"),
        "crash_file_count": total_crash_files,
        "hang_file_count": total_hang_files,
        "file_index": "file-index.tsv",
    }
    journal_path = write_journal(args, out_dir)
    if journal_path:
        manifest["journal_file"] = journal_path

    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    write_tsv(out_dir / "run-index.tsv", records)

    notes = args.notes or "ONNX AFL++ Arm C black-box campaign summary"
    summary = f"""# Arm C AFL++ Campaign Summary

## Conditions
- experiment_id: `{args.experiment_id}`
- machine: `{args.machine_label}`
- target: `{args.target}`
- backend: `{args.backend}` black-box AFL++ `-n`
- campaign_start: `{args.campaign_start or iso_or_empty(first_timestamp)}`
- campaign_end: `{args.campaign_end or iso_or_empty(last_timestamp)}`
- duration_hours: `{args.duration_hours}`
- runs_root: `{args.runs_root}`
- retention_root: `{args.retention_root}`
- corpus_dir: `{args.corpus_dir}`
- seed_count: `{manifest["seed_count"]}`
- workers: `{args.workers}`
- timeout_sec: `{args.timeout_sec}`
- restart_limit: `{args.restart_limit}`
- git_commit: `{manifest["git_commit"]}`

## Metrics
| key | value |
| --- | ---: |
| run_records | {len(records)} |
| live_records | {live_count} |
| retention_records | {archive_count} |
| total | {manifest["total"]} |
| success | {manifest["success"]} |
| failed | {manifest["failed"]} |
| timeout | {manifest["timeout"]} |
| retries | {manifest["retries"]} |
| fuzzer_stats_files | {stats_count} |
| execs_done_sum | {manifest["execs_done_sum"]} |
| corpus_count_max | {manifest["corpus_count_max"]} |
| saved_crashes_sum | {manifest["saved_crashes_sum"]} |
| saved_hangs_sum | {manifest["saved_hangs_sum"]} |
| crash_file_count | {total_crash_files} |
| hang_file_count | {total_hang_files} |

## Interpretation Guardrails
- This is an AFL++ black-box/dumb-mode backend summary.
- `execs_done_sum` is an AFL++ per-invocation execution proxy, not target-library coverage.
- `corpus_count_max` is an AFL++ queue/corpus proxy from short independent invocations.
- `success/failed/timeout` are orchestrator status counts from `status.json`.
- Compare Arm C to Arm A/B only with the backend-comparison caveats in the handoff plan.

## Notes
- {notes}
"""
    (out_dir / "summary.md").write_text(summary, encoding="utf-8")
    (out_dir / "notes.md").write_text(f"- note: {notes}\n", encoding="utf-8")
    file_index_path = write_file_index(
        out_dir,
        [
            "manifest.json",
            "summary.md",
            "run-index.tsv",
            "notes.md",
            "journal.txt",
        ],
    )

    print("[export-armc-aflpp] done")
    print(f"out_dir: {out_dir}")
    print(f"records: {len(records)}")
    print(f"success/failed/timeout: {manifest['success']}/{manifest['failed']}/{manifest['timeout']}")
    print(f"run_index: {out_dir / 'run-index.tsv'}")
    print(f"file_index: {file_index_path}")
    print(f"summary: {out_dir / 'summary.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
