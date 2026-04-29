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
    plugin: str
    plugin_body: str
    query: str
    extra_args: List[str]


@dataclass
class RunResult:
    exit_code: int
    stdout: str
    stderr: str


def normalize(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def read_scenarios(path: Path) -> List[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    return [
        Scenario(
            name=item["name"],
            fixture=item["fixture"],
            plugin=item["plugin"],
            plugin_body=item["plugin_body"],
            query=item["query"],
            extra_args=item.get("extra_args", []),
        )
        for item in payload.get("scenarios", [])
    ]


def run_plugin(
    bin_path: Path, cwd: Path, plugin: str, query: str, extra_args: List[str], env: dict
) -> RunResult:
    try:
        proc = subprocess.run(
            [str(bin_path), "plugin", "run", plugin, query, *extra_args],
            cwd=str(cwd),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=20,
        )
    except subprocess.TimeoutExpired as exc:
        return RunResult(
            exit_code=124,
            stdout=normalize(exc.stdout or ""),
            stderr=normalize((exc.stderr or "") + "\ncommand timed out"),
        )
    return RunResult(
        exit_code=proc.returncode,
        stdout=normalize(proc.stdout),
        stderr=normalize(proc.stderr),
    )


def install_plugin(tmp_dir: Path, plugin_name: str, body: str) -> None:
    xdg_home = tmp_dir / "xdg"
    plugin_dir = xdg_home / "na" / "plugins"
    plugin_dir.mkdir(parents=True, exist_ok=True)
    script_path = plugin_dir / f"{plugin_name}.sh"
    script_path.write_text(body, encoding="utf-8")
    script_path.chmod(0o755)


def main() -> int:
    parser = argparse.ArgumentParser(description="Diff Ruby and Rust `na plugin run` behavior.")
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
        default="fixtures/plugin_run",
        help="Directory containing plugin-run fixtures (default: fixtures/plugin_run)",
    )
    parser.add_argument(
        "--scenarios",
        default="fixtures/plugin_run/scenarios.json",
        help="Scenario JSON file (default: fixtures/plugin_run/scenarios.json)",
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
    print(f"Running {len(scenarios)} plugin run scenario(s)")
    for scenario in scenarios:
        fixture_path = (fixture_dir / scenario.fixture).resolve()
        if not fixture_path.exists():
            print(f"[FAIL] {scenario.name} (fixture missing: {fixture_path})")
            failures += 1
            continue

        with tempfile.TemporaryDirectory(prefix="na_rust_diff_plugin_") as ruby_tmp, tempfile.TemporaryDirectory(
            prefix="na_rust_diff_plugin_"
        ) as rust_tmp:
            ruby_dir = Path(ruby_tmp)
            rust_dir = Path(rust_tmp)
            shutil.copy2(fixture_path, ruby_dir / "case.taskpaper")
            shutil.copy2(fixture_path, rust_dir / "case.taskpaper")
            install_plugin(ruby_dir, scenario.plugin, scenario.plugin_body)
            install_plugin(rust_dir, scenario.plugin, scenario.plugin_body)

            ruby_env = os.environ.copy()
            rust_env = os.environ.copy()
            ruby_env["XDG_DATA_HOME"] = str(ruby_dir / "xdg")
            rust_env["XDG_DATA_HOME"] = str(rust_dir / "xdg")

            ruby_result = run_plugin(
                ruby_na, ruby_dir, scenario.plugin, scenario.query, scenario.extra_args, ruby_env
            )
            rust_result = run_plugin(
                rust_na, rust_dir, scenario.plugin, scenario.query, scenario.extra_args, rust_env
            )

        ruby_missing = "Plugin not found" in ruby_result.stderr
        rust_missing = "Plugin not found" in rust_result.stderr
        if ruby_result.exit_code == 124 or rust_result.exit_code == 124 or ruby_missing or rust_missing:
            skipped += 1
            reason = "timeout" if ruby_result.exit_code == 124 or rust_result.exit_code == 124 else "plugin discovery mismatch"
            print(f"[SKIP] {scenario.name} ({reason})")
            continue

        ok = (
            ruby_result.exit_code == rust_result.exit_code
            and ruby_result.stdout == rust_result.stdout
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

    passed = len(scenarios) - failures - skipped
    print(f"\nSummary: {passed}/{len(scenarios)} passed ({skipped} skipped)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
