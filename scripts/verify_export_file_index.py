#!/usr/bin/env python3
import argparse
import csv
import hashlib
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify an export file-index.tsv against files in the export directory."
    )
    parser.add_argument("export_dir", help="Export directory containing file-index.tsv")
    parser.add_argument("--index", default="file-index.tsv", help="Index file name or path")
    return parser.parse_args()


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as input_file:
        for chunk in iter(lambda: input_file.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def resolve_index_path(export_dir: Path, index_arg: str) -> Path:
    index_path = Path(index_arg)
    if index_path.is_absolute():
        return index_path
    return export_dir / index_path


def resolve_artifact_path(export_dir: Path, relative_path: str) -> Path:
    candidate = export_dir / relative_path
    resolved_export_dir = export_dir.resolve()
    resolved_candidate = candidate.resolve()
    if resolved_candidate == resolved_export_dir or resolved_export_dir not in resolved_candidate.parents:
        raise ValueError(f"relative_path escapes export_dir: {relative_path}")
    return resolved_candidate


def verify_index(export_dir: Path, index_path: Path) -> int:
    failures: list[str] = []
    rows_checked = 0

    with index_path.open("r", encoding="utf-8", newline="") as input_file:
        reader = csv.DictReader(input_file, delimiter="\t")
        required = {"relative_path", "size_bytes", "sha256"}
        missing = required.difference(reader.fieldnames or [])
        if missing:
            print(f"[verify-export] missing columns: {', '.join(sorted(missing))}", file=sys.stderr)
            return 2

        for row_number, row in enumerate(reader, start=2):
            relative_path = row.get("relative_path", "")
            expected_size = row.get("size_bytes", "")
            expected_sha256 = row.get("sha256", "")
            rows_checked += 1
            try:
                artifact_path = resolve_artifact_path(export_dir, relative_path)
            except ValueError as error:
                failures.append(f"line {row_number}: {error}")
                continue
            if not artifact_path.is_file():
                failures.append(f"line {row_number}: missing file: {relative_path}")
                continue
            actual_size = artifact_path.stat().st_size
            actual_sha256 = sha256_file(artifact_path)
            if str(actual_size) != expected_size:
                failures.append(
                    f"line {row_number}: size mismatch for {relative_path}: "
                    f"expected={expected_size} actual={actual_size}"
                )
            if actual_sha256 != expected_sha256:
                failures.append(
                    f"line {row_number}: sha256 mismatch for {relative_path}: "
                    f"expected={expected_sha256} actual={actual_sha256}"
                )

    if failures:
        for failure in failures:
            print(f"[verify-export] {failure}", file=sys.stderr)
        print(f"[verify-export] failed: rows_checked={rows_checked}", file=sys.stderr)
        return 1

    print(f"[verify-export] ok: rows_checked={rows_checked}")
    return 0


def main() -> int:
    args = parse_args()
    export_dir = Path(args.export_dir)
    if not export_dir.is_dir():
        print(f"[verify-export] export_dir not found: {export_dir}", file=sys.stderr)
        return 2
    index_path = resolve_index_path(export_dir, args.index)
    if not index_path.is_file():
        print(f"[verify-export] index not found: {index_path}", file=sys.stderr)
        return 2
    return verify_index(export_dir, index_path)


if __name__ == "__main__":
    raise SystemExit(main())
