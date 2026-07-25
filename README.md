# na

A fast command line tool for managing **next actions** in plain-text TaskPaper todo files. This is a Rust rewrite of the [na gem](https://github.com/ttscoff/na_gem), aimed at full behavioral parity with the Ruby version.

## Why NA?

Software projects accumulate todos in many places: issue trackers, sticky notes, random markdown files, and comments buried in code. When you sit down to work, the friction of finding *what to do next* in the current project often exceeds the friction of doing the work itself.

**na** ("next action") solves that by treating a simple TaskPaper file in your project directory as the source of truth. Actions tagged `@na` (or your custom tag) are your next actions. Run `na` in any directory and you immediately see what needs attention. Add items from the shell without opening an editor, mark them done, move them between projects, search across files, and archive finished work -- all without leaving the terminal.

Because todo files are plain text, they work with git, diff, sync, and any editor. **na** adds structure on top: discovery of `*.taskpaper` files in the tree, saved searches, prompt hooks that run when you `cd` into a project, plugins for custom output, and time tracking via `@started` / `@done` tags.

## Installation

**Homebrew (recommended on macOS):**

```bash
brew tap ttscoff/thelab
brew install ttscoff/thelab/na
```

**Cargo:**

```bash
cargo install na_rust
```

Or build from source:

```bash
git clone https://github.com/ttscoff/na_rust.git
cd na_rust
cargo install --path .
```

The binary is named `na`.

## Quick start

```bash
# List next actions in the current directory (default command)
na

# Add a new action to the Inbox
na add "Fix login bug @na"

# Find actions matching text
na find "login"

# Mark an action done
na update "Fix login" --done

# Show help for a subcommand
na next --help
```

## Global options

These flags can appear before any subcommand.

| Flag | Description |
|------|-------------|
| `-v`, `--version` | Print version and exit |
| `-e`, `--extension`, `--ext` | File extension for todo files (default: `taskpaper`) |
| `-g`, `--global-file` | Use a single global todo file instead of scanning the cwd |
| `--no-color` | Disable colorized output |
| `--color` | Force color even when stdout is not a TTY |
| `--include_ext` | Include file extension in displayed filenames |
| `--pager` / `--no-pager` | Paginate long output through `$PAGER` (default: on when stdout is a TTY) |
| `--repo-top` | Use `{git_root}/{repo_name}.{ext}` as the global todo file |
| `-t`, `--na_tag` | Tag marking a next action (default: `na`) |
| `--add_at` | Add new/moved entries at `start` or `end` of project (default: `start`) |
| `-d`, `--depth` | Default recursion depth when discovering todo files |
| `--template` | Template content for newly created todo files |
| `--cwd_as` | With `--global-file`, treat cwd as `project`, `tag`, or `none` (default: `none`) |
| `--debug` | Print verbose debug info on stderr |

**Deprecated globals** (Ruby compatibility):

| Flag | Description |
|------|-------------|
| `-a`, `--add` | Shorthand for `na add` (multi-word unknown commands rewrite to add) |
| `-r`, `--recurse` | Recurse 3 directories deep when command depth is unset |
| `-n`, `--note` | Prompt for notes when using legacy `-a`/`--add` |
| `-p`, `--priority` | Set priority when using legacy `-a`/`--add` |

Running `na` with no subcommand is equivalent to `na next`.

Bare titles dispatch to saved searches: `na "Weekly Focus"` runs `na saved run "Weekly Focus"`.

## Commands

### `next`, `show`

Show next actions (tagged with `@na` by default, excluding `@done`).

```
na next [FILTER] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `FILTER` | Optional `@search(...)` expression or filter text |
| `-a`, `--first-available`, `--available` | Show only the first available action per project |
| `--file` | Restrict to a specific todo file |
| `--all` | Include actions from all known todo files |
| `--hidden` | Include hidden directories when traversing |
| `-d`, `--depth` | Recurse to depth when discovering files |
| `--in`, `--todo` | Restrict to known todo files matching tokens |
| `--done` | Include completed actions |
| `-t`, `--tag` | Alternate next-action tag |
| `--project`, `--proj` | Filter by project name |
| `--tagged` | Match actions containing tag expressions |
| `-p`, `--priority`, `--prio` | Match by priority value/comparison |
| `--search`, `--find`, `--grep` | Additional text filter (repeatable) |
| `--regex` | Treat search as regular expression |
| `--exact` | Exact phrase match |
| `--search-notes` / `--no-search-notes` | Include/exclude notes while searching (default: include) |
| `--notes` / `--no-notes` | Include/exclude notes in output |
| `--no-file` | Omit filename prefix in output |
| `--nest` | Group output by todo file |
| `--omnifocus` | Nested by file and project (OmniFocus-style trees) |
| `--plugin` | Run a plugin on results (stdout only) |
| `--input`, `--output`, `--divider` | Plugin I/O format (`json`, `yaml`, `csv`, `text`) |
| `--times` | Show per-action durations and total |
| `--human` | Human-friendly duration strings |
| `--only-timed` | Only actions with both `@started` and `@done` |
| `--json-times` | Output timing data as JSON |
| `--only-times` | Output only elapsed totals |
| `--save` | Save this search definition for later reuse |

### `find`, `grep`, `search`

Search for actions by text or `@search(...)` expression.

```
na find [QUERY] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Search terms or `@search(...)` (default: empty) |
| `-e`, `--regex` | Regular expression match |
| `-x`, `--exact` | Exact phrase match |
| `-d`, `--depth` | Recurse to depth when discovering files |
| `--in`, `--todo` | Restrict to known todo files |
| `--search-notes` / `--no-search-notes` | Include/exclude notes while searching |
| `-o`, `--or` | Combine search tokens with OR |
| `--project`, `--proj` | Restrict by project |
| `--tagged` | Restrict by tag(s) |
| `--done` | Include completed actions |
| `-v`, `--invert` | Invert match results |
| `--save` | Save query as a named search |
| `--notes` / `--no-notes` | Include/exclude notes in output |
| `--nest` / `--omnifocus` | Nested output modes |
| `--no-file` | Omit filename in output |
| `--times`, `--human`, `--only-timed`, `--json-times`, `--only-times` | Time output (same as `next`) |
| `--plugin`, `--input`, `--output`, `--divider` | Plugin integration |

### `tagged`

Find actions matching tag expressions. Accepts all `find` flags; tag filters are positional arguments after flags.

```
na tagged [OPTIONS] [@tag ...]
```

Example: `na tagged --nest @na @idea`

### `add`

Add a new action to a todo file.

```
na add TEXT [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `TEXT` | Action text (parenthetical at end becomes a note) |
| `--started` | Started time (natural language or ISO) |
| `--end`, `--finished` | End/finished time |
| `--duration` | Duration (e.g. `45m`, `2h`, `1d2h30m`) |
| `--to`, `--project`, `--proj` | Target project (default: `Inbox`) |
| `--at` | Insert at start or end of project |
| `--in`, `--todo` | Add to a known todo file (partial match) |
| `-p`, `--priority` | Priority 1-5 or `h`/`m`/`l` |
| `-t`, `--tag` | Tag other than default next-action tag |
| `-x` | Do not add the next-action tag |
| `-f`, `--file` | Exact file path |
| `--finish`, `--done` | Mark new action as done |
| `-d`, `--depth` | Search depth for todo files |
| `-n`, `--note` | Prompt for additional notes (stdin used when piped) |

### `update`

Update, complete, tag, move, or delete existing actions.

```
na update [QUERY] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Keywords, `@search(...)`, or `PATH:LINE` (1-based line number) |
| `-t`, `--tag` | Tag(s) to add |
| `-r`, `--remove` | Tag(s) to remove |
| `-f`, `--finish`, `--done` | Mark as done |
| `--file` | Restrict to a specific file |
| `-d`, `--depth` | Search depth (default: 1) |
| `--in`, `--todo` | Restrict to known todo files |
| `--search`, `--find`, `--grep` | Additional filter terms |
| `--all` | Act on all matches (skip menu) |
| `--replace` | Replace action text |
| `--to`, `--move` | Move to another project |
| `--project`, `--proj` | Restrict by project |
| `--tagged` | Restrict by tag(s) |
| `-e`, `--regex` / `-x`, `--exact` | Match mode |
| `--search-notes` / `--no-search-notes` | Include/exclude notes while searching |
| `-p`, `--priority` | Set priority |
| `--at` | Insert/move position |
| `-a`, `--archive` | Archive selected actions |
| `--edit` | Open matched action in `$EDITOR` |
| `--editor` | Editor override |
| `--delete` | Delete selected actions |
| `--restore` | Restore from Archive to Inbox |
| `--note` | Append note(s) |
| `-o`, `--overwrite-notes`, `--overwrite` | Replace notes instead of appending |
| `--started`, `--end`, `--finished`, `--duration` | Set timing tags |
| `--plugin`, `--input`, `--output`, `--divider` | Run plugin and persist results |

### `edit`

Open matched actions in `$EDITOR` for multi-action editing (Ruby-compatible `# ------ path:line` separators). After the editor exits, changes are written back to the todo file(s).

```
na edit [QUERY] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Action selector (required unless `--tagged` / `--search` is used) |
| `--text` | Replace action text directly instead of opening an editor |
| `--file` | Restrict to a specific file |
| `-d`, `--depth` | Search depth (default: 1) |
| `--in`, `--todo` | Restrict to known todo files |
| `--search`, `--find`, `--grep` | Additional filter |
| `--tagged` | Restrict by tag(s) |
| `--done` | Include completed actions |
| `-e`, `--regex` / `-x`, `--exact` | Match mode |
| `--search-notes` / `--no-search-notes` | Include/exclude notes while searching |
| `--editor` | Editor override |
| `--all` | Edit all matches (skip menu) |

Examples:

```bash
na edit "login bug"          # select match(es), open in $EDITOR
na edit "login" --text "Fixed login @na"   # non-interactive replace
```

### `complete`, `finish`

Mark actions complete. Alias for `update --done`.

```
na complete [QUERY] [OPTIONS]
```

Accepts the same flags as `update`.

### `restore`, `unfinish`

Restore completed actions. Alias for `update --restore`.

### `archive`

Mark actions done and move them to the Archive project.

```
na archive [QUERY] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Optional action selector |
| `--done` | Archive all already-done tasks |
| `--file` | Restrict to a specific file |
| `-d`, `--depth` | Search depth (default: 1) |
| `--tagged` | Restrict by tag(s) |
| `--project`, `--proj` | Restrict by project |
| `--in`, `--todo` | Restrict to known todo files |
| `--search`, `--find`, `--grep` | Additional filter |
| `-e`, `--regex` / `-x`, `--exact` | Match mode |
| `--all` | Act on all matches without menu |
| `-n`, `--note` | Append note(s) to archived actions |
| `-o`, `--overwrite` | Replace existing notes |

### `completed`, `finished`

Display completed (`@done`) actions.

```
na completed [PATTERN ...] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `PATTERN` | Optional text patterns to match |
| `-b`, `--before` | Completed before date/time |
| `--on` | Completed on date |
| `-a`, `--after` | Completed after date/time |
| `-o`, `--or` | OR semantics for date ranges |
| `-d`, `--depth` | Search depth |
| `--in`, `--todo` | Restrict to known todo files |
| `--notes` / `--no-notes` | Include/exclude notes in output |
| `--search-notes` / `--no-search-notes` | Include/exclude notes while searching |
| `--project`, `--proj` | Filter by project |
| `--tagged` | Filter by tag(s) |
| `--nest` / `--omnifocus` | Nested output |
| `--no-file` | Omit filename in output |
| `--save` | Save this query |

### `move`

Move actions to another project.

```
na move [QUERY] --to PROJECT [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Action selector |
| `--to`, `--move` | Destination project (required) |
| `--at` | Insert at start or end |
| `--from` | Restrict source project |
| `--in`, `--todo` | Restrict to known todo files |
| `--file` | Restrict to a specific file |
| `-d`, `--depth` | Search depth (default: 1) |
| `--search`, `--find`, `--grep` | Additional filter |
| `--search-notes` | Include notes while searching |
| `--tagged` | Restrict by tag(s) |
| `-e`, `--regex` / `-x`, `--exact` | Match mode |
| `--all` | Move all matches |

### `tag`

Add tags to matching actions (tags are positional arguments).

```
na tag [@tag ...] [QUERY] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `@tag ...` | Tags to add |
| `QUERY` | Action selector |
| `--in`, `--todo` | Restrict to known todo files |
| `--done` | Include completed actions |
| `--file` | Restrict to a specific file |
| `-d`, `--depth` | Search depth (default: 1) |
| `--tagged` | Restrict by tag(s) |
| `-e`, `--regex` / `-x`, `--exact` | Match mode |
| `--search`, `--find`, `--grep` | Additional filter |
| `--search-notes` | Include notes while searching |
| `--all` | Tag all matches |

### `open`

Open todo files in an editor or application.

```
na open [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `--in`, `--todo` | Open known todo files matching tokens |
| `-d`, `--depth` | Search depth (default: 1) |
| `-e`, `--editor` | Editor command |
| `-a`, `--app` | macOS application to open files |

### `projects`

List projects in todo files.

```
na projects [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-d`, `--depth` | Search depth (default: 1) |
| `-p`, `--paths` | Show full project paths |

### `todos`

List known todo files from the discovery registry (`tdlist.txt` under the NA data directory). Every time `na` finds todo files in the current tree (or you run `na scan`), their absolute paths are recorded so you can list or target them from anywhere.

```
na todos [QUERY ...] [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `QUERY` | Optional fuzzy path tokens (`na todos marked`, `na todos code/marked`) |
| `-e`, `--edit` | Open the registry file itself in `$EDITOR` |

### `undo`

Restore the most recent backup of a todo file.

```
na undo [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-s`, `--select`, `--choose` | Interactively choose a backup |

### `scan`

Scan the directory tree and register todo files.

```
na scan [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-d`, `--depth` | Recursion depth (default: 3) |
| `-p`, `--prune` | Remove registry entries for missing files |
| `--hidden` | Include hidden directories |
| `-n`, `--dry-run` | Show what would change without writing |

### `init`, `create`

Create a new todo file in the current directory, named after the optional project
argument (or the git repo / directory name). Uses the built-in blank template
(or `--template` when set).

```
na init
na init warpspeed
```

### `prompt`

Manage shell prompt hooks that display next actions when entering a directory.

```
na prompt [SUBCOMMAND]
```

| Subcommand | Description |
|------------|-------------|
| `show` | Print the prompt hook script for your current shell |
| `install` | Install the hook into your shell profile |

See [Prompt hooks](#prompt-hooks) below for setup details.

### `changes`, `changelog`

Display `CHANGELOG.md` (paginated when stdout is a TTY).

```
na changes
```

### `saved`

Manage saved search definitions stored under the data directory.

```
na saved SUBCOMMAND [OPTIONS]
```

| Subcommand | Description |
|------------|-------------|
| `list` | List saved search titles |
| `run TITLE` | Run a saved search |
| `edit TITLE` | Edit a saved search (`--editor` to override) |
| `delete TITLE` | Delete a saved search |
| `select` | Interactively pick and run a saved search |

### `initconfig`, `init-config`

Write current global CLI options to `na.rc`.

```
na initconfig [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `--force` | Overwrite an existing config file |

Config is read from (first found): `$XDG_CONFIG_HOME/na/na.rc`, `$XDG_CONFIG_HOME/na.rc`, or `~/.na.rc`.

### `plugin`

Inspect and run user plugins from the plugins directory.

```
na plugin SUBCOMMAND [OPTIONS]
```

| Subcommand | Description |
|------------|-------------|
| `list` | List discovered plugins |
| `run PLUGIN [QUERY]` | Run plugin against matching actions and apply results to files |
| `enable PLUGIN` | Make plugin executable |
| `disable PLUGIN` | Remove executable permission |
| `new PLUGIN` | Create a new plugin stub |
| `edit PLUGIN` | Open plugin in editor (`--editor` to override) |
| `generate-examples` | Generate example plugin fixtures |

`plugin run` flags: `--file`, `-d`/`--depth`, `--in`/`--todo`, `--search`/`--find`/`--grep`, `--done`, `--tagged`, `--input`, `--output`, `--divider`.

Plugins live in `$XDG_DATA_HOME/na/plugins` (or `~/.local/share/na/plugins`).

## Prompt hooks

You can add a prompt command to your shell to have **na** automatically list your next actions when you `cd` into a directory. To install a prompt command for your current shell, run:

```bash
na prompt install
```

It works with Zsh, Bash, and Fish. If you'd rather edit your startup file yourself, run `na prompt show` to print the hook and where to add it.

If you're using a single global file (`--global-file`), a prompt hook requires `--cwd_as` to be `tag` or `project`. **na** will list actions based on the current directory name — matching either a project or a tag, depending on your setting.

Add `-r` to recurse three directories deep on each `cd`, or set a default depth in `na.rc`.

After installing a hook, start a new terminal session (or `source` your profile) to activate it.

**Zsh example** (what `na prompt show` generates):

```bash
# zsh prompt hook for na
chpwd() { na next }
```

With a global file and project-based filtering:

```bash
na --global-file ~/todo.taskpaper --cwd_as project prompt install
```

## Configuration

Global defaults can be persisted with:

```bash
na --global-file ~/todo.taskpaper --na_tag na initconfig
```

Supported `na.rc` fields: `ext`, `file`, `na_tag`, `add_at`, `depth`, `template`, `no_color`, `cwd_as`.

## Theme

Output formatting is controlled by `theme.yaml`, loaded from:

1. `~/.local/share/na/theme.yaml`
2. `$XDG_DATA_HOME/na/theme.yaml`

**Colors** use brace syntax: `{bc}`, `{g}`, `{#eccc87}`, `{b#hex}`.

**Templates** for flat output use placeholders: `%filename`, `%line`, `%parents`, `%project`, `%action`, `%note`. With `--nest` or `--omnifocus`, output uses `path:line:` banners and project trees instead of flat templates.

## TaskPaper searches

`na` understands TaskPaper-style `@search(...)` expressions in `next`, `find`, and saved searches. Supported features include tag predicates, project filters, boolean `and`/`or`, item paths (`/Project//Sub`), wildcards, regex `matches`, and result slices (`[n]`, `[a:b]`). See the [Ruby na documentation](https://github.com/ttscoff/na_gem) for the full search syntax reference.

## Time tracking

Actions with `@started` and `@done` tags support duration reporting:

```bash
na next --times              # DD:HH:MM:SS per action + total
na next --times --human      # "2h 15m" style
na next --json-times         # JSON summary
na tagged @na --only-times   # Totals only
```

## Development

### Differential runners

Ruby-vs-Rust parity checks:

```bash
cargo build
python3 scripts/diff_next.py
python3 scripts/diff_find.py
python3 scripts/diff_tagged.py
python3 scripts/diff_completed.py
python3 scripts/diff_update.py
python3 scripts/diff_archive.py
python3 scripts/diff_plugin_run.py
```

Optional harness flags: `--ruby-na`, `--rust-na`, `--fixtures`, `--scenarios`.

### Parity checklist (living)

Use this list as the current parity status against `na_gem`.

#### Implemented

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
- [x] `next` / `find` / `completed` flat action lines: `theme.yaml` layouts (`templates.output` overrides when set; else `default` / `single_file` / `multi_file` / `no_file`), color tokens (`parent`, `bracket`, `project`, `action`, ...), Ruby-style `%filename` / `%line` / `%parents` / `%project` / `%note` (including abutted tokens), and `--no-file`; leaf project in brackets for `%parents%`
- [x] `find` baseline via shared query engine
- [x] Mutation fixture coverage for `update --done` and tag add/remove
- [x] Backup path moved to XDG-aware data dir (`$XDG_DATA_HOME` / fallback)
- [x] Shared XDG path helpers for data/config path resolution (`src/io/xdg.rs`)
- [x] Plugin input serialization helpers (`json`, `yaml`, `csv`, `text-divider`)
- [x] Initial Ruby-vs-Rust differential harness for `next`
- [x] Time output parity (flat `next` / `find` / `tagged`): `--times`, `--human`, `--json-times`, `--only-times`, `--only-timed`
- [x] CLI / alias sweep: global `--extension` alias `--ext`, `next -a` (`--first-available`), `update -r` / `-f` / `-o`, long `--finish` (with `--done` alias), `--overwrite` alias on `--overwrite-notes`, and `move --move` (alias for `--to`)

#### Command compatibility matrix

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

#### Known gaps

- [ ] `next` / `find` / `completed` nested polish (theme.duration-style brackets on timed nested lines)
- [ ] Residual flat action edge cases (report with a fixture if they diverge from the gem)
- [x] Archive filtering parity in current differential fixtures
- [x] Ruby globals / legacy shims (`--color`, `--pager`, `--include_ext`, `--repo-top`, `-r`, `-a`, `--debug`). Remaining: persisting `pager` / `include_ext` / `color` in `initconfig` / `na.rc`
- [x] Full `@search(...)` parity for core Ruby clause parsing
- [ ] Mutation semantics parity beyond current `update` baseline (interactive editor paths still partial)
- [x] Differential harness expansion to mutation commands

When parity changes: add fixtures, run the diff scripts above, and update this checklist.
