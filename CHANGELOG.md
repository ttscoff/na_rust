### 0.1.0

2026-06-29 08:34

#### CHANGED

- `plugin run` applies plugin output to todo files instead of printing JSON to stdout
- `plugin run` QUERY positional is optional; omit it to run against all actions in the file (Ruby-compatible)
- Saved search missing-file errors use Ruby-style "Search {title} not found"

#### NEW

- Add `--color` global flag to force colorized output when stdout is not a TTY
- Add `--include_ext` global flag to include file extensions in multi-file display labels
- Add `--pager` / `--no-pager` globals to paginate long action lists through $PAGER (skips output under 2000 chars or 50 lines; disabled when stdout is not a TTY)
- Add `--repo-top` global to use `{git_root}/{repo_name}.{ext}` as the global todo file, prompting to create it when missing (uses `--template` when set)
- Dispatch bare `na "Title"` and `na saved "Title"` to saved search run (Ruby unknown-command behavior)
- Rewrite legacy `na -a WORD ...` to `na add` when the command is unknown (Ruby `-a`/`--add` shim)

#### IMPROVED

- Action list output for next, find, completed, nest, and time summaries is buffered and paginated as a unit
- `--omnifocus` disables pagination to match Ruby
- `diff_plugin_run.py` installs Ruby plugins under HOME/.local/share (Ruby ignores XDG_DATA_HOME) and compares post-run file content


