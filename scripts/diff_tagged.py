#!/usr/bin/env python3
"""Diff Ruby and Rust `na tagged` outputs (same flags as find + time modes)."""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import List


@dataclass
class Scenario:
    name: str
    fixture: str
    tags: List[str]
    extra_args: List[str]


@dataclass
class RunResult:
    exit_code: int
    stdout: str
    stderr: str


def read_scenarios(path: Path) -> List[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    out = []
    for item in payload.get("scenarios", []):
        tags = item.get("tags")
        if tags is None:
            tags = item.get("tag", [])
            if isinstance(tags, str):
                tags = [tags]
        out.append(
            Scenario(
                name=item["name"],
                fixture=item["fixture"],
                tags=list(tags),
                extra_args=item.get("args", []),
            )
        )
    return out


def normalize(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def normalize_times_line_spacing(text: str) -> str:
    lines = []
    for line in text.split("\n"):
        lines.append(re.sub(r"(\]\s*:\d+)\s{2,}", r"\1 ", line))
    return "\n".join(lines)


def normalize_json_times_stdout(text: str) -> str:
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        return text

    def to_utc_z(s: str) -> str:
        dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        else:
            dt = dt.astimezone(timezone.utc)
        return dt.strftime("%Y-%m-%dT%H:%M:%SZ")

    for row in data.get("timed", []):
        for key in ("started", "ended"):
            if key in row and isinstance(row[key], str):
                row[key] = to_utc_z(row[key])
    return json.dumps(data, indent=2, sort_keys=True)


def postprocess_tagged_stdout(stdout: str, extra_args: List[str]) -> str:
    if "--json-times" in extra_args:
        return normalize_json_times_stdout(stdout)
    return normalize_times_line_spacing(stdout)


def run_cmd(command: List[str], cwd: Path) -> RunResult:
    env = os.environ.copy()
    env.setdefault("TZ", "UTC")
    proc = subprocess.run(
        command,
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    return RunResult(proc.returncode, normalize(proc.stdout), normalize(proc.stderr))


def main() -> int:
    parser = argparse.ArgumentParser(description="Diff Ruby and Rust `na tagged` outputs.")
    parser.add_argument(
        "--ruby-na",
        default=os.path.expanduser("~/Desktop/Code/na_gem/bin/na"),
        help="Path to Ruby na executable",
    )
    parser.add_argument(
        "--rust-na",
        default="./target/debug/na",
        help="Path to Rust na executable",
    )
    parser.add_argument(
        "--fixtures",
        default="fixtures/tagged",
        help="Directory containing taskpaper fixture files",
    )
    parser.add_argument(
        "--scenarios",
        default="fixtures/tagged/scenarios.json",
        help="Scenario JSON file",
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
    print(f"Running {len(scenarios)} tagged scenario(s)")
    for scenario in scenarios:
        fixture_path = (fixture_dir / scenario.fixture).resolve()
        if not fixture_path.exists():
            print(f"[FAIL] {scenario.name} (fixture missing: {fixture_path})")
            failures += 1
            continue

        with tempfile.TemporaryDirectory(prefix="na_rust_diff_tagged_") as tmp:
            tmp_path = Path(tmp)
            tmp_fixture = tmp_path / "case.taskpaper"
            shutil.copy2(fixture_path, tmp_fixture)
            ruby_cmd = [str(ruby_na), "tagged"] + scenario.extra_args + scenario.tags
            rust_cmd = [str(rust_na), "tagged"] + scenario.extra_args + scenario.tags
            ruby_result = run_cmd(ruby_cmd, tmp_path)
            rust_result = run_cmd(rust_cmd, tmp_path)

        ruby_out = postprocess_tagged_stdout(ruby_result.stdout, scenario.extra_args)
        rust_out = postprocess_tagged_stdout(rust_result.stdout, scenario.extra_args)

        ok = (
            ruby_result.exit_code == rust_result.exit_code
            and ruby_out == rust_out
            and ruby_result.stderr == rust_result.stderr
        )
        print(f"[{'PASS' if ok else 'FAIL'}] {scenario.name}")
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
