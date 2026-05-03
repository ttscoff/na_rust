# na_rust Parity Next Steps

## Goal

Reach practical feature parity with `~/Desktop/Code/na_gem` for day-to-day CLI usage,
while preserving behavior for existing TaskPaper files and plugin workflows.

For a **living** command/feature checklist, see **`README.md`** (parity checklist,
command matrix, known gaps, and `theme.yaml` notes).

---

## Where we are now (progress snapshot)

Relative to the original audit below, large pieces are in place:

- **Commands:** The CLI mirrors Ruby’s surface area for core workflows (`next`, `find`,
  `tagged`, `add`, `update`, `complete`, `archive`, `completed`, `move`, `tag`,
  `saved`, `plugin`, `open`, `projects`, `todos`, `scan`, `init`, `prompt`,
  `changes`, `undo`, and more). `tagged` delegates to `find` (all find flags,
  including `--no-file`, apply).
- **`@search(...)`:** Core clause parsing is aligned with Ruby (wildcards, groups,
  item paths, `project` shortcut, `matches`, relation modifiers, trailing slices, etc.),
  with tests and parser coverage.
- **Flat action output:** `theme.yaml` is loaded (gem path + XDG `na` data dir),
  color templates (`{…}` / hex), and layout templates including `templates.output`
  (overrides mode-specific templates when set), `default` / `single_file` /
  `multi_file` / `no_file`, Ruby-style abutted tokens (`%filename%line`, …),
  `%project` (theme key `project`), `%note` inline, bracket/parent split, and
  `--no-file` on `next` / `find` / `completed`.
- **Differential harness:** Scripts exist for `next`, `find`, `completed`, `update`,
  `archive`, and `plugin run` (see `README.md` and `scripts/diff_*.py`).
- **Backups / paths:** XDG-style data dir helpers; backups under the na data directory.
- **CLI flag / alias pass:** Common Ruby shims on `next` / `update` / `move` / globals
  (e.g. `--ext`, `next -a`, `update -r/-f/-o`, `--finish` / `--overwrite` long forms,
  `move --move`) with parse tests; see README checklist.
- **Time output (flat mode):** `--times`, `--human`, `--json-times`, `--only-times`,
  and `--only-timed` match Ruby’s duration strings (`DD:HH:MM:SS` and human phrasing),
  JSON shape (`timed` / `tags` / `total`), markdown tag table + `Total time:` line,
  full `format_action` lines with optional `[duration]` suffix; `@start` accepted
  like `@started`; `find` / `tagged` expose the same time flags (delegation).

**Still clearly behind or partial vs the gem** (see README “Known Gaps”):

- **`--nest` / `--omnifocus`:** Match Ruby’s **`path:line:`** headers, tag/bracket
  theming, OmniFocus trees, and `$COLUMNS` wrap on bodies; see **`diff_next`**
  nest/omnifocus scenarios. With time flags, **`--json-times` still emits JSON**;
  non-JSON time decoration is skipped on nested action lines (same as the gem).
- **Action line:** Tag stripping/formatting beyond **`@na`** may still diverge.
- **`update` / mutations:** Non-interactive baseline + fixtures; interactive /
  legacy delegation paths and full semantic parity are partial.
- **Globals:** Remaining Ruby global / deprecated shim gaps (see README).
- **Config files:** Ruby-style `na.rc` / multi-path config resolution is not a
  stated focus in Rust yet (XDG config **dir** exists; rc file policy TBD).
- **Time output QA:** Add **`fixtures/*/scenarios.json`** rows for `--times`,
  `--json-times`, `--only-times`, etc., driving **`diff_next.py`** /
  **`diff_find.py`** — tracked as **prioritized item 4** below (supplements
  `src/output/duration.rs` unit tests).
- **Plugins:** List/run/enable/new/edit and merge formats; applying every Ruby
  plugin **action** type to disk may still be partial.

---

## Original audit reference (source: `na_gem`)

Reviewed: `na_gem/bin/na`, `na_gem/bin/commands/`, and tests such as
`next_action_test`, `next_available_actions_test`, `taskpaper_search_item_path_test`,
`depth_test`, `time_output_test`, `time_tracking_test`, `plugins_test`,
`filename_indicator_test`, `todo_test`.

The numbered areas below are kept as a **reference**. Use the **Prioritized next
steps** section for what to do next; many bullets under each section are already
addressed (see snapshot above).

### 1) Command and Flag Parity Matrix

- **Largely done:** Broad command/alias coverage; `next --available` ↔
  `--first-available`; many `next`/`find` targeting flags exist (`--file`,
  `--in`, `--depth`, `--all`, `--hidden`, etc.—verify against Ruby for edge flags).
  Recent pass added common short/long mirrors for `update`, `move`, `next`,
  `--ext`, `--finish`/`-f`, etc. (README).
- **Remaining:** Ruby **global** flags and legacy shims (README “Known Gaps”);
  tighten differential scenarios so mismatches are intentional.

### 2) `@search(...)` Full Semantics

- **Largely done:** Matches, modifiers, item paths, project shortcut, slices—see README.
- **Remaining:** Edge cases uncovered by running Ruby differential harnesses or new fixtures.

### 3) Output Formatting Parity

- **Flat lists:** Theme + templates + cwd-style filename labels—implemented.
- **Nested:** `--nest` / `--omnifocus` implemented as plain structured output;
  theme integration and `$COLUMNS` wrap on nested lines remain **open**.
- **Notes:** Flat mode honors `%note` and `--notes`; nested uses `*` when notes hidden.

### 4) Time Tracking and Time Output Modes

- **Largely done (flat stdout):** Duration formatting, per-action suffixes, footer /
  markdown table, JSON payload, `@start` / `@started`, plugin ordering before summary,
  `find` / `tagged` flags — see README and `src/output/duration.rs`.
- **Remaining:** Optional differential harness scenarios for timing; theme-colored
  duration brackets like Ruby (`theme.duration`) not wired for the suffix/footer.

### 5) Mutation Workflow Parity (`update` and `complete`)

- **Partial:** Core mutations + fixtures; interactive flows and full Ruby semantics
  remain **open**.

### 6) Plugin System Parity

- **Partial:** Discovery, formats, `plugin run`, merge back into actions; full action-type
  application and metadata parity may remain **open**.

### 7) File Discovery and Selection Behavior

- **Largely done:** Depth/hidden patterns, multiple files; interactive menus with
  non-interactive fallbacks in many paths.
- **Remaining:** Match Ruby edge cases from `depth_test` / discovery tests via harness.

### 8) XDG Path and Config Parity

- **Data:** XDG data dir + `na` subtree used for backups, themes, plugins, saved searches.
- **Config:** Config dir helper exists; **Ruby-style rc file chain** (`na.rc` paths)
  still a **parity gap** if we want file-based defaults matching the gem.

### 9) Differential Harness Expansion

- **Done for baseline:** Multiple commands have `diff_*.py` runners (see README).
- **Ongoing:** Add scenarios as parity improves; normalize Ruby/Rust paths in CI.

---

## Recently completed

- **CLI compatibility sweep:** High-traffic Ruby aliases / short flags and help-visible
  long forms documented in README; parse tests in `cli.rs`.
- **Time output parity (flat):** Ruby-matched duration formatting, JSON, markdown
  summary, full action listing with optional duration suffixes; extended to `find` /
  `tagged` (`--only-timed`, `--json-times`, `--only-times`).
- **Differential time scenarios:** `fixtures/next`, `find`, and `tagged` include timed
  TaskPaper fixtures; `diff_next.py`, `diff_find.py`, and `diff_tagged.py` run with
  **`TZ=UTC`** and normalize spacing/JSON so CI can catch duration and JSON regressions.
- **Nested `next` parity:** **`--nest`** and **`--omnifocus`** match Ruby’s **`path:line:`**
  banners, OmniFocus trees (tabs/`@tags`), and **`diff_next`** scenarios with stable path
  normalization.

## Prioritized next steps

Ordered roughly by impact for “feels like the gem” / reducing surprise:

1. **Nested output polish** — **Done (baseline):** `path:line:` banners, OmniFocus
   indentation/tree parity, `$COLUMNS` wrap for nested bodies, and **`diff_next`**
   scenarios for **`--nest`** / **`--omnifocus`**. **Remaining:** optional
   `theme.duration`-style time suffixes on nested lines (if desired), and more
   fixtures for edge cases.
2. **Flat action text parity** — Strip/format tags beyond `@na` to match Ruby in the
   relevant modes (fixture-driven).
3. **`update` / mutation depth** — Close gaps on interactive editor paths,
   `PATH:LINE` edge cases, and mutation ordering vs Ruby where fixtures exist.
4. **Plugin action applications** — Ensure plugin-returned operations (`MOVE`,
   `ADD_TAG`, …) match Ruby behavior where the runner claims support.
5. **Config file resolution** — If desired: implement Ruby-compatible `na.rc` /
   config lookup (`$XDG_CONFIG_HOME/na/…`, fallbacks) and document intentional diffs.
6. **Continuous differential hygiene** — Run `scripts/diff_*.py` after substantive
   changes; extend scenario JSON when fixing a parity bug.

Smaller follow-ups:

- **Globals / residual CLI shims** — Anything still listed under README “Known Gaps”
  for Ruby globals; add intentional differential notes where behavior differs on purpose.

Maintenance items: expand `find`/`completed` scenarios in the harness, align `saved`
search storage paths with Ruby if users rely on interchangeability.

---

## Release gates

- Differential parity runs documented per command group (`next`, `find`, `tagged`,
  `completed`, `update`, `archive`, `plugin run`) where applicable.
- No critical regressions in mutation safety (backups + file content assertions in tests).
- CLI help and **README** parity sections updated when behavior changes.
