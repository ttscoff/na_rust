# na_rust Parity Next Steps

## Goal

Reach practical feature parity with `~/Desktop/Code/na_gem` for
day-to-day CLI usage, while preserving behavior for existing
TaskPaper files and plugin workflows.

## Current Baseline (Completed)

- Binary name is `na`.
- Nested project parsing and parent chain tracking implemented.
- Action note block parsing implemented.
- `@search(...)` has `beginswith` and `endswith`; explicit `!=`
  and regex tests exist.
- `find` baseline uses shared query engine.
- `update` supports add/remove tag and done mutation fixtures.
- Backup path uses XDG-style data location with shared path helpers.
- Plugin serialization supports `json`, `yaml`, `csv`, and
  text-divider mode.
- Initial differential harness exists for `next`
  (`scripts/diff_next.py`).

## New Requirements From Ruby CLI and Tests Audit

Source reviewed:

- `na_gem/bin/na` and command files under `na_gem/bin/commands/`.
- Tests under `na_gem/test/*`, especially:
  `next_action_test`, `next_available_actions_test`,
  `taskpaper_search_item_path_test`, `depth_test`,
  `time_output_test`, `time_tracking_test`, `plugins_test`,
  `filename_indicator_test`, and `todo_test`.

### 1) Command and Flag Parity Matrix

- Build and enforce a command matrix for `next`, `find`, `add`,
  `update`, `complete`, and `plugin`.
- Add Ruby-compatible aliases where practical:
  - `next --available` alias for Rust `--first-available`
  - command aliases such as `next/show` and `find/grep/search`.
- Add support for Ruby-style file targeting parity:
  - `next --file`, `next --in`, `next --all`,
    `next --hidden`, `next --depth`.

### 2) `@search(...)` Full Semantics

- Implement missing behavior shown in Ruby:
  - `matches` operator mapping
  - relation modifiers (`[i]`, etc.) and case handling
  - item-path expressions (for example `/Inbox//Bugs`)
  - `project` shortcut (`@search(project Inbox)`)
  - slice semantics (`[index]`, `[start:end]`) on results.
- Add dedicated fixtures mirroring Ruby item-path and slice tests.

### 3) Output Formatting Parity

- Implement Ruby-style default rendering differences:
  - filename/project/line indicator shape (`[Project] :line ...`)
  - multi-file formatting and cwd indicator behavior (`./`)
    when subdirectory files are present.
- Port nesting modes:
  - `--nest`
  - `--omnifocus` (nested by file and project).
- Port optional note display behavior
  (`--notes` and `--no-notes` equivalents).

### 4) Time Tracking and Time Output Modes

- Add and port started/done/duration mutation semantics:
  - `@started(...)`, `@done(...)`, duration-derived timestamps.
- Add output modes:
  - `--times`
  - `--human`
  - `--only-timed`
  - `--only-times`
  - `--json-times`.
- Add parity fixtures for markdown table totals and JSON schema.

### 5) Mutation Workflow Parity (`update` and `complete`)

- Expand update semantics to cover:
  - move/to project behavior including project path creation
  - archive/restore/delete
  - replace text
  - note append vs overwrite
  - `PATH:LINE` style direct targeting.
- Ensure stable multi-action mutation ordering.

### 6) Plugin System Parity

- Support plugin metadata keys in Ruby style:
  `name/title`, `input`, `output`.
- Add plugin action parity from returned payload:
  - `UPDATE`, `MOVE`, `ADD_TAG`, `DELETE_TAG`,
    `COMPLETE`, `RESTORE`, `ARCHIVE`, `DELETE`.
- Add plugin management command parity:
  - `plugin list`, `plugin run`, `plugin enable/disable`,
    `plugin new`, `plugin edit`.
- Keep format round-trip compatibility for
  `json`, `yaml`, `csv`, and text mode.

### 7) File Discovery and Selection Behavior

- Match depth and hidden discovery behavior from Ruby tests.
- Add known-todo and history matching behavior parity.
- Track interactive selection behavior (`fzf` and `gum`)
  with non-interactive safe fallback.

### 8) XDG Path and Config Parity

- Keep all data/config references on env-driven XDG resolution.
- Add Ruby-compatible config lookup behavior:
  - `$XDG_CONFIG_HOME/na/na.rc`
  - `$XDG_CONFIG_HOME/na.rc`
  - `~/.na.rc` fallback for compatibility.
- Ensure parity for saved search/database locations under data dir.

### 9) Differential Harness Expansion

- Expand differential harness beyond `next`:
  - `find` read-only scenarios
  - `update` and `complete` mutation scenarios
  - plugin transform scenarios.
- Normalize expected differences via per-scenario adapters only
  where required (for example flag-name translation).

## Prioritized Next 12 Tasks

1. Add `next --available` alias and update harness scenarios
   to remove alias mismatch noise.
2. Port Ruby default `next` output template shape
   (project/line and tag rendering behavior).
3. Add item-path filter support for `@search(...)`
   (`/Project//Descendant`) with fixtures.
4. Add `project` shortcut and expression slice support
   to `@search(...)`.
5. Add `next` flags: `--depth`, `--hidden`, `--all`, and `--file`.
6. Add `find` differential scenarios and baseline report output.
7. Implement update move/archive/restore/delete parity fixtures
   with exact file snapshots.
8. Add started/done/duration mutation support
   in CLI flows and core models.
9. Implement `--times`, `--only-times`, `--only-timed`,
   and `--json-times` output modes.
10. Extend plugin runner to apply action-type operations
    (`MOVE`, `ADD_TAG`, `DELETE_TAG`, etc.).
11. Implement saved-search storage path and minimal
    `saved` command parity.
12. Add config-file resolution parity
    (XDG paths plus `~/.na.rc` fallback).

## Release Gates

- Differential parity report published per command group:
  `next`, `find`, `update`, `plugin`.
- No critical regressions in mutation safety
  (backups plus final file text assertions).
- CLI help/docs updated to reflect parity aliases
  and intentional differences.
