#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List


@dataclass
class Scenario:
    name: str
    fixture: str
    args: List[str]


@dataclass
class RunResult:
    exit_code: int
    stdout: str
    stderr: str
    file_after: str


def normalize(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def read_scenarios(path: Path) -> List[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    return [
        Scenario(
            name=item["name"],
            fixture=item["fixture"],
            args=item["args"],
        )
        for item in payload.get("scenarios", [])
    ]


def run_update(bin_path: Path, args: List[str], cwd: Path, fixture_name: str) -> RunResult:
    try:
        proc = subprocess.run(
            [str(bin_path), "update", *args],
            cwd=str(cwd),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=20,
        )
    except subprocess.TimeoutExpired as exc:
        return RunResult(
            exit_code=124,
            stdout=normalize((exc.stdout or "")),
            stderr=normalize((exc.stderr or "") + "\ncommand timed out"),
            file_after=normalize((cwd / fixture_name).read_text(encoding="utf-8")),
        )
    file_after = normalize((cwd / fixture_name).read_text(encoding="utf-8"))
    return RunResult(
        exit_code=proc.returncode,
        stdout=normalize(proc.stdout),
        stderr=normalize(proc.stderr),
        file_after=file_after,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Diff Ruby and Rust `na update` behavior.")
    parser.add_argument(
        "--ruby-na",
        default=os.path.expanduser("~/Desktop/Code/na_gem/bin/na"),
        help="Path to Ruby na executable (default: ~/Desktop/Code/na_gem/bin/na)",
    )
    parser.add_argument(
        "--rust-na",
        default="./target/debug/na",
        help="Path to Rust na executable (default: ./target/debug/na)",
    )
    parser.add_argument(
        "--fixtures",
        default="fixtures/update",
        help="Directory containing update fixtures (default: fixtures/update)",
    )
    parser.add_argument(
        "--scenarios",
        default="fixtures/update/scenarios.json",
        help="Scenario JSON file (default: fixtures/update/scenarios.json)",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    ruby_na = Path(args.ruby_na).expanduser()
    rust_na = Path(args.rust_na)
    if not rust_na.is_absolute():
        rust_na = (repo_root / rust_na).resolve()
    fixture_dir = Path(args.fixtures)
    if not fixture_dir.is_absolute():
        fixture_dir = (repo_root / fixture_dir).resolve()
    scenarios_path = Path(args.scenarios)
    if not scenarios_path.is_absolute():
        scenarios_path = (repo_root / scenarios_path).resolve()

    if not ruby_na.exists():
        print(f"Ruby na not found at {ruby_na}", file=sys.stderr)
        return 2
    if not rust_na.exists():
        print(f"Rust na not found at {rust_na}. Run `cargo build` first.", file=sys.stderr)
        return 2
    if not scenarios_path.exists():
        print(f"Scenario file not found at {scenarios_path}", file=sys.stderr)
        return 2

    scenarios = read_scenarios(scenarios_path)
    if not scenarios:
        print("No scenarios found; nothing to compare.", file=sys.stderr)
        return 2

    failures = 0
    skipped = 0
    print(f"Running {len(scenarios)} update scenario(s)")
    for scenario in scenarios:
        fixture_path = (fixture_dir / scenario.fixture).resolve()
        if not fixture_path.exists():
            print(f"[FAIL] {scenario.name} (fixture missing: {fixture_path})")
            failures += 1
            continue

        with tempfile.TemporaryDirectory(prefix="na_rust_diff_update_") as ruby_tmp, tempfile.TemporaryDirectory(
            prefix="na_rust_diff_update_"
        ) as rust_tmp:
            ruby_dir = Path(ruby_tmp)
            rust_dir = Path(rust_tmp)
            fixture_name = "case.taskpaper"
            shutil.copy2(fixture_path, ruby_dir / fixture_name)
            shutil.copy2(fixture_path, rust_dir / fixture_name)
            ruby_result = run_update(ruby_na, scenario.args, ruby_dir, fixture_name)
            rust_result = run_update(rust_na, scenario.args, rust_dir, fixture_name)

        if ruby_result.exit_code == 124 or rust_result.exit_code == 124:
            skipped += 1
            print(f"[SKIP] {scenario.name} (command timed out)")
            continue
        ok = (
            ruby_result.exit_code == rust_result.exit_code
            and ruby_result.stdout == rust_result.stdout
            and ruby_result.stderr == rust_result.stderr
            and ruby_result.file_after == rust_result.file_after
        )
        print(f"[{'PASS' if ok else 'FAIL'}] {scenario.name}")
        if not ok:
            failures += 1
            print("  ruby:")
            print(f"    exit={ruby_result.exit_code}")
            print(f"    stdout={ruby_result.stdout!r}")
            print(f"    stderr={ruby_result.stderr!r}")
            print(f"    file={ruby_result.file_after!r}")
            print("  rust:")
            print(f"    exit={rust_result.exit_code}")
            print(f"    stdout={rust_result.stdout!r}")
            print(f"    stderr={rust_result.stderr!r}")
            print(f"    file={rust_result.file_after!r}")

    passed = len(scenarios) - failures - skipped
    print(f"\nSummary: {passed}/{len(scenarios)} passed ({skipped} skipped)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
