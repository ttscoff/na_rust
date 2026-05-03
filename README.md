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
- [x] `@search(...)` wildcard support for terms and `project` predicates (`*`)
- [x] `@search(...)` parenthesized grouping support for nested `and`/`or` clauses
- [x] `@search(...)` item-path expressions (`/A/B`, `//descendant`, `*`, optional `path` prefix; same slash semantics as add/update item paths)
- [x] `@search(...)` trailing result slices (`[n]`, `[a:b]`, `[:b]`, `[a:]`, `[:]`) applied per OR clause after filtering (matches Ruby `evaluate_taskpaper_search`)
- [x] `next` / `find` / `completed` flat action lines: `theme.yaml` layouts (`templates.output` overrides when set; else `default` / `single_file` / `multi_file` / `no_file`), color tokens (`parent`, `bracket`, `project`, `action`, …), Ruby-style `%filename` / `%line` / `%parents` / `%project` / `%note` (including abutted tokens), and `--no-file`; leaf project in brackets for `%parents%`
- [x] `find` baseline via shared query engine
- [x] Mutation fixture coverage for `update --done` and tag add/remove
- [x] Backup path moved to XDG-aware data dir (`$XDG_DATA_HOME` / fallback)
- [x] Shared XDG path helpers for data/config path resolution (`src/io/xdg.rs`)
- [x] Plugin input serialization helpers (`json`, `yaml`, `csv`, `text-divider`)
- [x] Initial Ruby-vs-Rust differential harness for `next`
- [x] **Time output parity (flat `next` / `find` / `tagged`):** `--times`,
  `--human`, `--json-times`, `--only-times`, `--only-timed` aligned with Ruby
  `NA::Actions` behavior: `DD:HH:MM:SS` and human duration strings, JSON
  (`timed` / `tags` / `total`), markdown tag table + `Total time:` line when
  applicable, per-action lines from normal templates with optional `[duration]`
  suffix; `@start` accepted as well as `@started`; plugin merge runs before time
  summary (same order as the gem). Nested mode: non-JSON time flags follow the
  gem’s pattern (no inline duration decoration on nested lines; `--json-times`
  still emits JSON).
- [x] **CLI / alias sweep (recent):** Help-visible parity for common Ruby options,
  including global `--extension` alias `--ext`, `next -a` (`--first-available`),
  `update -r` / `-f` / `-o`, long `--finish` (with `--done` alias), `--overwrite`
  alias on `--overwrite-notes`, and `move --move` (alias for `--to`). Parse tests
  live under `cli.rs`; remaining gaps are mainly **Ruby globals** / deprecated
  shims (tracked below).

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

### Theme (`theme.yaml`)

Rust loads and merges YAML from (in order, later overrides earlier):

1. `~/.local/share/na/theme.yaml` (same path as the Ruby gem)
2. `$XDG_DATA_HOME/na/theme.yaml` (on macOS without `XDG_DATA_HOME`, typically `~/Library/Application Support/na/theme.yaml`)

**Colors:** brace syntax compatible with Ruby, e.g. `{bc}`, `{g}`, `{#eccc87}`, `{b#…}` for background hex — see `src/output/color_template.rs`.

**Templates:** For **flat** stdout (not `--nest`), if `templates.output` is **non-empty** it becomes the layout string for all modes (same role Ruby gives the `output` slot when calling `Action.pretty`). Otherwise Rust picks `multi_file` / `no_file` / `single_file` / `default` like Ruby’s mode selection.

Placeholders match Ruby’s token set: `%filename` / `%filename%`, `%line` / `%line%`, `%parents` / `%parent` / `%parents%`, **`%project`** (leaf name only; theme key `project`), `%action`, `%note`. Ruby often **abuts** tokens without spaces (e.g. `%filename%line` — one `%` between tokens); that form is supported. Rust may also use explicit `%filename%%line%` between segments.

**Nested output (`--nest` / `--omnifocus`):** grouping and indentation match the Ruby-style shapes (per-file header, tabbed `- [a/b/c]` lines, or OmniFocus-style project trees), but nested mode is **plain text** — **`theme.yaml` colors and templates do not apply** there today. When notes exist but are omitted, action lines still get a trailing `*` (same idea as flat output).

### Known Gaps

- [ ] **`next` / `find` / `completed` nested mode (`--nest` / `--omnifocus`)**
  (structure and `*` note markers align with Ruby-style output; **no `theme.yaml` colors/templates**, **no `$COLUMNS` word-wrap** on nested lines; minor file-path header differences vs Ruby possible. Combined with **`--times` / `--human` / `--only-times`** Ruby does not decorate nested lines — Rust mirrors that; **`--json-times` still emits JSON** when nested.)
- [x] Project label in action output uses leaf project (matches Ruby brackets)
- [ ] Action text rendering parity beyond `@na` strip
  (Ruby may strip or format other tags differently in some modes)
- [x] Archive filtering parity in current differential fixtures
- [ ] **Ruby globals / leftover option shims**
  (e.g. parity for all legacy globals in `na_gem/bin/na` — `--color` vs `--no-color`-only,
  pager/recurse/tag globals, etc. Subcommands above reflect the recent alias pass.)
- [x] Full `@search(...)` parity for core Ruby clause parsing (wildcards, groups, item paths, slices)
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

**Note:** Time-output behavior is covered by unit tests in `src/output/duration.rs`
and integration through `na next` / `na find` / `na tagged`. CI parity for timing
flags vs Ruby is planned as **prioritized item 4** in **`NEXT_STEPS.md`**: add
scenarios to `fixtures/next/scenarios.json` and `fixtures/find/scenarios.json`
for `diff_next.py` / `diff_find.py`.
