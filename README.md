# na_rust

## Differential Runner (`next`)

Run Ruby-vs-Rust parity checks for `next` fixtures:

```bash
cargo build
python3 scripts/diff_next.py
```

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
- [x] `find` baseline via shared query engine
- [x] Mutation fixture coverage for `update --done` and tag add/remove
- [x] Backup path moved to XDG-aware data dir (`$XDG_DATA_HOME` / fallback)
- [x] Shared XDG path helpers for data/config path resolution (`src/io/xdg.rs`)
- [x] Plugin input serialization helpers (`json`, `yaml`, `csv`, `text-divider`)
- [x] Initial Ruby-vs-Rust differential harness for `next`

### Known Gaps

- [ ] `next` output formatting parity
  (Ruby includes `[Project] :line` style output)
- [ ] Action text rendering parity
  (Ruby strips some tags from default display)
- [ ] Archive filtering parity for `next` defaults
  (Ruby fixture currently includes an Archive item where Rust excludes it)
- [ ] Ruby option/alias compatibility gaps
  (`--available` vs `--first-available`, and more)
- [ ] Full `@search(...)` parity
  (`matches`, wildcard behavior, regex variants, nested precedence checks)
- [ ] Mutation semantics parity beyond current `update` baseline
  (move/delete/restore/note overwrite)
- [ ] Differential harness expansion to `find` and mutation commands

### Updating This Checklist

When parity changes:

1. Add or update a fixture and differential scenario.
2. Run `python3 scripts/diff_next.py`.
3. Update checklist items based on actual pass/fail evidence.
