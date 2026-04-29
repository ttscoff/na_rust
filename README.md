# na_rust

## Differential Runners

Supported: `next`, `find`, `completed`, `update`, `archive`, `plugin run`.

Run Ruby-vs-Rust parity checks for `next` fixtures:

```bash
cargo build
python3 scripts/diff_next.py
python3 scripts/diff_find.py
python3 scripts/diff_completed.py
python3 scripts/diff_update.py
python3 scripts/diff_archive.py
python3 scripts/diff_plugin_run.py
```

Note: Ruby `na` can block in non-interactive mutation commands and may differ in
plugin discovery paths on some systems. `diff_update.py`, `diff_archive.py`,
and `diff_plugin_run.py` treat those cases as `SKIP` so the suite remains
usable in CI.

Optional flags:

- `--ruby-na` path to Ruby `na` executable
- `--rust-na` path to Rust `na` executable
- `--fixtures` fixture directory
- `--scenarios` scenario JSON file

## Parity Checklist (Living)

Use this list as the current parity status against `na_gem`.

### Implemented

- [x] Nested TaskPaper project hierarchy parsing with parent-chain tracking
- [x] Action note block parsing
- [x] `@search(...)` support for `beginswith` and `endswith`
- [x] Explicit `!=` and regex match test coverage
- [x] `@search(...)` support for `matches` operator and `project NAME` shortcut
- [x] Case modifier support for string/regex relations (`[i]`)
- [x] `find` baseline via shared query engine
- [x] Mutation fixture coverage for `update --done` and tag add/remove
- [x] Backup path moved to XDG-aware data dir (`$XDG_DATA_HOME` / fallback)
- [x] Shared XDG path helpers for data/config path resolution (`src/io/xdg.rs`)
- [x] Plugin input serialization helpers (`json`, `yaml`, `csv`, `text-divider`)
- [x] Initial Ruby-vs-Rust differential harness for `next`

### Command Compatibility Matrix

- `next` / `show`: implemented
- `add`: implemented
- `update`: implemented (expanded mutation/selection flags)
- `edit`: implemented
- `find` / `grep` / `search`: implemented (expanded flag surface)
- `tagged`: implemented
- `complete` / `finish`: implemented
- `restore` / `unfinish`: implemented
- `archive`: implemented
- `completed` / `finished`: implemented
- `move`: implemented
- `tag`: implemented
- `saved`: implemented (list/run/edit/delete/select)
- `plugin`: implemented (`list/run/new/edit/enable/disable`)
- `open`: implemented
- `projects`: implemented
- `todos`: implemented
- `scan`: implemented
- `init` / `create`: implemented
- `prompt`: implemented (`show/install`)
- `changes` / `changelog`: implemented
- `undo`: implemented

### Known Gaps

- [ ] Full `next` output rendering parity
  (core shape matches fixtures, but edge-formatting differences may remain)
- [x] Project label in action output uses leaf project (matches Ruby brackets)
- [ ] Action text rendering parity beyond `@na` strip
  (Ruby may strip or format other tags differently in some modes)
- [x] Archive filtering parity in current differential fixtures
- [ ] Remaining Ruby option/alias compatibility gaps
  (global deprecated compatibility shims are still partial)
- [ ] Full `@search(...)` parity
  (`project` globs with `*` for chain/segment matching implemented;
  item-path expressions, slice semantics, nested precedence checks still open)
- [ ] Mutation semantics parity beyond current `update` baseline
  (interactive editor and legacy delegation paths still partial)
- [x] Differential harness expansion to mutation commands
  (`update`, `archive`, and `plugin run` scripts present)

### Updating This Checklist

When parity changes:

1. Add or update a fixture and differential scenario.
2. Run `python3 scripts/diff_next.py`, `python3 scripts/diff_find.py`,
   `python3 scripts/diff_completed.py`, `python3 scripts/diff_update.py`,
   `python3 scripts/diff_archive.py`, and `python3 scripts/diff_plugin_run.py`.
3. Update checklist items based on actual pass/fail evidence.
