#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import List


@dataclass
class Scenario:
    name: str
    fixture: str
    args: List[str]
    ruby_args: List[str]
    rust_args: List[str]


@dataclass
class RunResult:
    exit_code: int
    stdout: str
    stderr: str


def read_scenarios(path: Path) -> List[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    scenarios = []
    for item in payload.get("scenarios", []):
        scenarios.append(
            Scenario(
                name=item["name"],
                fixture=item["fixture"],
                args=item.get("args", []),
                ruby_args=item.get("ruby_args", []),
                rust_args=item.get("rust_args", []),
            )
        )
    return scenarios


def run_cmd(command: List[str], cwd: Path) -> RunResult:
    proc = subprocess.run(
        command,
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return RunResult(proc.returncode, normalize(proc.stdout), normalize(proc.stderr))


def normalize(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def render_report_line(ok: bool, scenario: Scenario) -> str:
    status = "PASS" if ok else "FAIL"
    return f"[{status}] {scenario.name}"


def main() -> int:
    parser = argparse.ArgumentParser(description="Diff Ruby and Rust `na next` outputs.")
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
        default="fixtures/next",
        help="Directory containing taskpaper fixture files (default: fixtures/next)",
    )
    parser.add_argument(
        "--scenarios",
        default="fixtures/next/scenarios.json",
        help="Scenario JSON file (default: fixtures/next/scenarios.json)",
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
    print(f"Running {len(scenarios)} next scenario(s)")
    for scenario in scenarios:
        fixture_path = (fixture_dir / scenario.fixture).resolve()
        if not fixture_path.exists():
            print(f"[FAIL] {scenario.name} (fixture missing: {fixture_path})")
            failures += 1
            continue

        effective_ruby_args = scenario.ruby_args or scenario.args
        effective_rust_args = scenario.rust_args or scenario.args
        ruby_args = ["next", "--file", str(fixture_path)] + effective_ruby_args
        rust_args = ["next", "--file", str(fixture_path)] + effective_rust_args
        ruby_result = run_cmd([str(ruby_na)] + ruby_args, repo_root)
        rust_result = run_cmd([str(rust_na)] + rust_args, repo_root)

        same_exit = ruby_result.exit_code == rust_result.exit_code
        same_stdout = ruby_result.stdout == rust_result.stdout
        same_stderr = ruby_result.stderr == rust_result.stderr
        ok = same_exit and same_stdout and same_stderr
        print(render_report_line(ok, scenario))
        if not ok:
            failures += 1
            print("  ruby:")
            print(f"    exit={ruby_result.exit_code}")
            print(f"    stdout={ruby_result.stdout!r}")
            print(f"    stderr={ruby_result.stderr!r}")
            print("  rust:")
            print(f"    exit={rust_result.exit_code}")
            print(f"    stdout={rust_result.stdout!r}")
            print(f"    stderr={rust_result.stderr!r}")

    passed = len(scenarios) - failures
    print(f"\nSummary: {passed}/{len(scenarios)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
