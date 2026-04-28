#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import sys
import tempfile
import shutil
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


def read_scenarios(path: Path) -> List[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    return [
        Scenario(
            name=item["name"],
            fixture=item["fixture"],
            args=item.get("args", []),
        )
        for item in payload.get("scenarios", [])
    ]


def normalize(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def run_cmd(command: List[str], cwd: Path) -> RunResult:
    proc = subprocess.run(
        command,
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return RunResult(proc.returncode, normalize(proc.stdout), normalize(proc.stderr))


def main() -> int:
    parser = argparse.ArgumentParser(description="Diff Ruby and Rust `na completed` outputs.")
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
        default="fixtures/completed",
        help="Directory containing taskpaper fixture files (default: fixtures/completed)",
    )
    parser.add_argument(
        "--scenarios",
        default="fixtures/completed/scenarios.json",
        help="Scenario JSON file (default: fixtures/completed/scenarios.json)",
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

    if not ruby_na.exists() or not rust_na.exists() or not scenarios_path.exists():
        print("Missing executable or scenario inputs.", file=sys.stderr)
        return 2

    scenarios = read_scenarios(scenarios_path)
    if not scenarios:
        print("No scenarios found; nothing to compare.", file=sys.stderr)
        return 2

    failures = 0
    print(f"Running {len(scenarios)} completed scenario(s)")
    for scenario in scenarios:
        fixture_path = (fixture_dir / scenario.fixture).resolve()
        if not fixture_path.exists():
            print(f"[FAIL] {scenario.name} (fixture missing: {fixture_path})")
            failures += 1
            continue

        with tempfile.TemporaryDirectory(prefix="na_rust_diff_completed_") as tmp:
            tmp_path = Path(tmp)
            tmp_fixture = tmp_path / "case.taskpaper"
            shutil.copy2(fixture_path, tmp_fixture)
            ruby_result = run_cmd([str(ruby_na), "completed"] + scenario.args, tmp_path)
            rust_result = run_cmd([str(rust_na), "completed"] + scenario.args, tmp_path)
        # Ruby CLI currently exits non-zero for `completed` even on successful output.
        # Compare rendered output/stderr parity instead of exit status.
        ok = ruby_result.stdout == rust_result.stdout and ruby_result.stderr == rust_result.stderr
        print(f"[{'PASS' if ok else 'FAIL'}] {scenario.name}")
        if not ok:
            failures += 1
            print("  ruby:", ruby_result)
            print("  rust:", rust_result)

    passed = len(scenarios) - failures
    print(f"\nSummary: {passed}/{len(scenarios)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
