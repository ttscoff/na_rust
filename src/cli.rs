use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "na",
    disable_version_flag = true,
    about = "Rust rewrite of na TaskPaper CLI",
    long_about = None
)]
pub struct Cli {
    /// Print version and exit.
    #[arg(short = 'v', long = "version", default_value_t = false)]
    pub version: bool,

    /// Override file extension when searching for TaskPaper files.
    #[arg(short, long, visible_alias = "ext", default_value = "taskpaper")]
    pub extension: String,

    /// Work against a single global file instead of cwd scanning.
    #[arg(short = 'g', long)]
    pub global_file: Option<PathBuf>,

    /// Disable colorized output.
    #[arg(long, default_value_t = false, global = true)]
    pub no_color: bool,

    /// Colorize output (forces color even when stdout is not a TTY).
    #[arg(long, default_value_t = false, global = true)]
    pub color: bool,

    /// Include file extension in displayed filenames.
    #[arg(long = "include_ext", default_value_t = false, global = true)]
    pub include_ext: bool,

    /// Paginate long output when stdout is a TTY (default: enabled).
    #[arg(long, default_value_t = true, global = true)]
    pub pager: bool,

    /// Disable pagination.
    #[arg(long = "no-pager", default_value_t = false, global = true)]
    pub no_pager: bool,

    /// Use a todo file at the git repository root named after the repository.
    #[arg(long = "repo-top", default_value_t = false, global = true)]
    pub repo_top: bool,

    /// Tag to consider a next action.
    #[arg(short = 't', long = "na_tag", default_value = "na")]
    pub na_tag: String,

    /// Add new/moved entries at start or end of target project.
    #[arg(long = "add_at", default_value = "start")]
    pub add_at: String,

    /// Recurse to depth when discovering todo files (global default).
    #[arg(short = 'd', long = "depth")]
    pub depth: Option<usize>,

    /// Add a next action (deprecated global; use `na add`). Short `-a` is expanded in argv normalization.
    #[arg(long = "add", default_value_t = false, global = true)]
    pub add: bool,

    /// Recurse 3 directories deep (deprecated; use `-d 3`). Short `-r` is expanded in argv normalization.
    #[arg(long = "recurse", default_value_t = false, global = true)]
    pub recurse: bool,

    /// Display verbose debug output on stderr.
    #[arg(long = "debug", default_value_t = false, global = true)]
    pub debug: bool,

    /// Template for new/blank todo files.
    #[arg(long)]
    pub template: Option<String>,

    /// Use cwd as project, tag, or none when using a global file.
    #[arg(long = "cwd_as", default_value = "none")]
    pub cwd_as: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            version: false,
            extension: "taskpaper".to_string(),
            global_file: None,
            no_color: false,
            color: false,
            include_ext: false,
            pager: true,
            no_pager: false,
            repo_top: false,
            na_tag: "na".to_string(),
            add_at: "start".to_string(),
            depth: None,
            add: false,
            recurse: false,
            debug: false,
            template: None,
            cwd_as: "none".to_string(),
            command: None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show the next available actions.
    #[command(visible_alias = "show")]
    Next(NextArgs),
    /// Find actions by search terms.
    #[command(visible_aliases = ["grep", "search"])]
    Find(FindArgs),
    /// Find actions matching tag expressions.
    Tagged(TaggedArgs),
    /// Add a new action.
    Add(AddArgs),
    /// Update existing actions.
    Update(UpdateArgs),
    /// Edit action text/notes directly.
    Edit(EditArgs),
    /// Mark actions complete.
    #[command(visible_alias = "finish")]
    Complete(UpdateArgs),
    /// Mark actions done and move to Archive.
    Archive(ArchiveArgs),
    /// Display completed actions.
    #[command(visible_alias = "finished")]
    Completed(CompletedArgs),
    /// Restore completed actions (alias: unfinish).
    #[command(visible_alias = "unfinish")]
    Restore(UpdateArgs),
    /// Move actions to another project.
    Move(MoveArgs),
    /// Add/remove/replace tags on actions.
    Tag(TagArgs),
    /// Open todo files in editor.
    Open(OpenArgs),
    /// List projects in todo files.
    Projects(ProjectsArgs),
    /// List known todo files.
    Todos(TodosArgs),
    /// Undo last backup state.
    Undo(UndoArgs),
    /// Scan directory tree for todo files.
    Scan(ScanArgs),
    /// Initialize a new todo file.
    #[command(visible_alias = "create")]
    Init(InitArgs),
    /// Show prompt scripts.
    Prompt(PromptArgs),
    /// Show changelog.
    #[command(visible_alias = "changelog")]
    Changes,
    /// Manage saved `next` search definitions.
    Saved(SavedArgs),
    /// Initialize the config file using current global options.
    #[command(name = "initconfig", visible_alias = "init-config")]
    InitConfig(InitConfigArgs),
    /// Inspect or run plugins.
    Plugin(PluginArgs),
}

#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// Project name for the new todo file (defaults to git repo / directory name).
    #[arg(value_name = "PROJECT")]
    pub project: Vec<String>,
}

#[derive(Debug, Args)]
pub struct InitConfigArgs {
    /// Overwrite an existing config file.
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct NextArgs {
    /// Optional TaskPaper-style filter expression.
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,

    /// Keep only one action per project.
    #[arg(
        short = 'a',
        long = "first-available",
        visible_alias = "available",
        default_value_t = false
    )]
    pub first_available: bool,

    /// Display matches from a specific TaskPaper file.
    #[arg(long)]
    pub file: Option<PathBuf>,

    /// Display matches from all known todo files.
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Include hidden directories while traversing.
    #[arg(long, default_value_t = false)]
    pub hidden: bool,

    /// Recurse to depth when searching for files.
    #[arg(short = 'd', long)]
    pub depth: Option<usize>,

    /// Display matches from known todo files in history.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Include @done actions.
    #[arg(long, default_value_t = false)]
    pub done: bool,

    /// Alternate next-action tag.
    #[arg(short = 't', long, value_name = "TAG")]
    pub tag: Option<String>,

    /// Filter by project name.
    #[arg(long = "project", visible_alias = "proj", value_name = "PROJECT")]
    pub project: Option<String>,

    /// Match actions containing tag/value expressions.
    #[arg(long, value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Match actions by priority value/comparison.
    #[arg(short = 'p', long = "priority", visible_alias = "prio", value_name = "PRIORITY", num_args = 0..)]
    pub priority: Vec<String>,

    /// Filter results using text query (repeatable).
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,

    /// Treat search query as regex.
    #[arg(long, default_value_t = false)]
    pub regex: bool,

    /// Treat search query as exact phrase match.
    #[arg(long, default_value_t = false)]
    pub exact: bool,

    /// Include notes while searching.
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,

    /// Exclude notes while searching (pairs with --search-notes).
    #[arg(long = "no-search-notes", default_value_t = false)]
    pub no_search_notes: bool,

    /// Include notes in output.
    #[arg(long, default_value_t = false)]
    pub notes: bool,

    /// Omit notes from output (pairs with --notes).
    #[arg(long = "no-notes", default_value_t = false)]
    pub no_notes: bool,

    /// Omit filename prefix in output.
    #[arg(long = "no-file", default_value_t = false)]
    pub no_file: bool,

    /// Group output by todo file.
    #[arg(long, default_value_t = false)]
    pub nest: bool,

    /// Nested by file and by project.
    #[arg(long, default_value_t = false)]
    pub omnifocus: bool,

    /// Run plugin on resulting actions (stdout only).
    #[arg(long, value_name = "NAME")]
    pub plugin: Option<String>,

    /// Plugin stdin format (json|yaml|csv|text).
    #[arg(long, value_name = "TYPE")]
    pub input: Option<String>,

    /// Plugin stdout format (json|yaml|csv|text).
    #[arg(long, value_name = "TYPE")]
    pub output: Option<String>,

    /// Divider string for text-divider plugin I/O.
    #[arg(long, value_name = "STRING")]
    pub divider: Option<String>,

    /// Show per-action durations and total.
    #[arg(long, default_value_t = false)]
    pub times: bool,

    /// Format durations in human-friendly form.
    #[arg(long, default_value_t = false)]
    pub human: bool,

    /// Show only actions with both @started and @done.
    #[arg(long = "only-timed", default_value_t = false)]
    pub only_timed: bool,

    /// Output times as JSON object (implies --times and --done).
    #[arg(long = "json-times", default_value_t = false)]
    pub json_times: bool,

    /// Output only elapsed totals (implies --times and --done).
    #[arg(long = "only-times", default_value_t = false)]
    pub only_times: bool,

    /// Save this search definition under the data directory for later reuse.
    #[arg(long, value_name = "TITLE")]
    pub save: Option<String>,
}

impl NextArgs {
    pub fn effective_search_notes(&self) -> bool {
        self.search_notes && !self.no_search_notes
    }

    pub fn effective_notes(&self) -> bool {
        self.notes && !self.no_notes
    }

    pub fn nest_for_display(&self) -> bool {
        self.nest || self.omnifocus
    }
}

#[derive(Debug, Clone, Args)]
pub struct FindArgs {
    /// Search query terms or @search(...) expression.
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,

    /// Interpret search pattern as regular expression.
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,

    /// Match pattern exactly.
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,

    /// Recurse to depth when discovering files.
    #[arg(short = 'd', long = "depth")]
    pub depth: Option<usize>,

    /// Restrict to known todo files matching tokens.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Include notes while searching.
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,

    /// Exclude notes while searching.
    #[arg(long = "no-search-notes", default_value_t = false)]
    pub no_search_notes: bool,

    /// Combine search tokens with OR semantics.
    #[arg(short = 'o', long = "or", default_value_t = false)]
    pub or_mode: bool,

    /// Restrict by project.
    #[arg(long = "project", visible_alias = "proj", value_name = "PROJECT")]
    pub project: Option<String>,

    /// Restrict by tag(s) presence.
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Include done actions.
    #[arg(long, default_value_t = false)]
    pub done: bool,

    /// Invert match results.
    #[arg(short = 'v', long = "invert", default_value_t = false)]
    pub invert: bool,

    /// Save this query as a named search.
    #[arg(long, value_name = "TITLE")]
    pub save: Option<String>,

    /// Include notes in output.
    #[arg(long, default_value_t = false)]
    pub notes: bool,

    /// Omit notes in output.
    #[arg(long = "no-notes", default_value_t = false)]
    pub no_notes: bool,

    /// Group output by file.
    #[arg(long, default_value_t = false)]
    pub nest: bool,

    /// Omit filename in output.
    #[arg(long = "no-file", default_value_t = false)]
    pub no_file: bool,

    /// Group output by file and project.
    #[arg(long, default_value_t = false)]
    pub omnifocus: bool,

    /// Show per-action durations and total.
    #[arg(long, default_value_t = false)]
    pub times: bool,

    /// Format durations in human-friendly form.
    #[arg(long, default_value_t = false)]
    pub human: bool,

    /// Show only actions with both @started and @done.
    #[arg(long = "only-timed", default_value_t = false)]
    pub only_timed: bool,

    /// Output times as JSON object (implies including done actions; same schema as `next`).
    #[arg(long = "json-times", default_value_t = false)]
    pub json_times: bool,

    /// Output only the duration summary (markdown table / total).
    #[arg(long = "only-times", default_value_t = false)]
    pub only_times: bool,

    /// Run plugin on resulting actions.
    #[arg(long, value_name = "NAME")]
    pub plugin: Option<String>,

    /// Plugin stdin format.
    #[arg(long, value_name = "TYPE")]
    pub input: Option<String>,

    /// Plugin stdout format.
    #[arg(long, value_name = "TYPE")]
    pub output: Option<String>,

    /// Divider string for plugin I/O.
    #[arg(long, value_name = "STRING")]
    pub divider: Option<String>,
}

impl FindArgs {
    pub fn effective_search_notes(&self) -> bool {
        self.search_notes && !self.no_search_notes
    }

    pub fn effective_notes(&self) -> bool {
        self.notes && !self.no_notes
    }

    pub fn nest_for_display(&self) -> bool {
        self.nest || self.omnifocus
    }
}

#[derive(Debug, Args, Clone)]
pub struct TaggedArgs {
    #[command(flatten)]
    pub find: FindArgs,
    /// Tag filters (combined into the find query). Place **after** find flags, e.g. `na tagged --json-times @na`.
    #[arg(value_name = "TAG", num_args = 0.., last = true)]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Action text to append.
    #[arg(value_name = "TEXT")]
    pub text: String,

    /// Started time string (ISO or natural-language style token).
    #[arg(long, value_name = "DATE")]
    pub started: Option<String>,

    /// End/finished time string.
    #[arg(long = "end", visible_alias = "finished", value_name = "DATE")]
    pub end: Option<String>,

    /// Duration token (e.g. 45m, 2h).
    #[arg(long, value_name = "DURATION")]
    pub duration: Option<String>,

    /// Add action to specific project.
    #[arg(long = "to", visible_aliases = ["project", "proj"], value_name = "PROJECT", default_value = "Inbox")]
    pub project: String,

    /// Add task at start or end of target project.
    #[arg(long, value_name = "POSITION")]
    pub at: Option<String>,

    /// Add to a known todo file (partial match).
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Add a priority level 1-5 or h/m/l.
    #[arg(short = 'p', long = "priority", value_name = "PRIO")]
    pub priority: Option<String>,

    /// Use a tag other than default next-action tag.
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    pub tag: Option<String>,

    /// Don't add next-action tag to the new entry.
    #[arg(short = 'x', default_value_t = false)]
    pub no_next_tag: bool,

    /// Specify exact file path to add into.
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Mark task as done.
    #[arg(long = "finish", visible_alias = "done", default_value_t = false)]
    pub finish: bool,

    /// Search for files this many directories deep.
    #[arg(short = 'd', long = "depth")]
    pub depth: Option<usize>,

    /// Include stdin note lines when provided.
    #[arg(short = 'n', long = "note", default_value_t = false)]
    pub note: bool,
}

#[derive(Debug, Args, Clone)]
pub struct UpdateArgs {
    /// Search query: `@search(...)`, keywords, or **`PATH:LINE`** (1-based line, same as interactive menus).
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,

    /// Tags to add (e.g. @today @home).
    #[arg(short, long, value_name = "TAG", num_args = 0..)]
    pub tag: Vec<String>,

    /// Tags to remove (e.g. @today @home).
    #[arg(short = 'r', long, visible_alias = "remove", value_name = "TAG", num_args = 0..)]
    pub untag: Vec<String>,

    /// Mark action as done.
    #[arg(
        short = 'f',
        long = "finish",
        visible_alias = "done",
        default_value_t = false
    )]
    pub done: bool,

    /// Restrict updates to a specific file path.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Search for todo files this many levels deep.
    #[arg(short = 'd', long, default_value_t = 1)]
    pub depth: usize,

    /// Restrict updates to known todo files matching tokens.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Additional search terms used as a fallback filter.
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,

    /// Update all matches immediately (skip interactive selection).
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Replace action text for selected items.
    #[arg(long, value_name = "TEXT")]
    pub replace: Option<String>,

    /// Move selected actions to another project.
    #[arg(long = "to", visible_alias = "move", value_name = "PROJECT")]
    pub to: Option<String>,

    /// Restrict by project.
    #[arg(long = "project", visible_alias = "proj", value_name = "PROJECT")]
    pub project: Option<String>,

    /// Restrict by tag(s) presence.
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Treat query as regex.
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,

    /// Treat query as exact text.
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,

    /// Include notes while searching.
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,

    /// Exclude notes while searching.
    #[arg(long = "no-search-notes", default_value_t = false)]
    pub no_search_notes: bool,

    /// Set priority value.
    #[arg(short = 'p', long = "priority", value_name = "PRIORITY")]
    pub priority: Option<String>,

    /// Insert/move position hint.
    #[arg(long = "at", value_name = "POSITION")]
    pub at: Option<String>,

    /// Archive selected actions.
    #[arg(short = 'a', long = "archive", default_value_t = false)]
    pub archive: bool,

    /// Trigger edit semantics.
    #[arg(long = "edit", default_value_t = false)]
    pub edit: bool,

    /// Editor for `--edit` (overrides `NA_EDITOR`, then `GIT_EDITOR`, then `EDITOR`).
    #[arg(long = "editor", value_name = "EDITOR")]
    pub editor: Option<String>,

    /// Delete selected actions.
    #[arg(long, default_value_t = false)]
    pub delete: bool,

    /// Restore selected actions from Archive to Inbox.
    #[arg(long, default_value_t = false)]
    pub restore: bool,

    /// Append note(s) to selected actions.
    #[arg(long = "note", value_name = "NOTE", num_args = 0..)]
    pub note: Vec<String>,

    /// Replace existing notes instead of appending.
    #[arg(
        short = 'o',
        long = "overwrite-notes",
        visible_alias = "overwrite",
        default_value_t = false
    )]
    pub overwrite_notes: bool,

    /// Set started timestamp token.
    #[arg(long = "started", value_name = "DATE")]
    pub started: Option<String>,

    /// Set done timestamp token explicitly.
    #[arg(long = "end", visible_alias = "finished", value_name = "DATE")]
    pub end: Option<String>,

    /// Set duration tag value.
    #[arg(long = "duration", value_name = "DURATION")]
    pub duration: Option<String>,

    /// Run a plugin on selected actions and persist results (Ruby `update --plugin`).
    #[arg(long, value_name = "NAME")]
    pub plugin: Option<String>,

    /// Plugin stdin format (json|yaml|csv|text).
    #[arg(long, value_name = "TYPE")]
    pub input: Option<String>,

    /// Plugin stdout format (json|yaml|csv|text).
    #[arg(long, value_name = "TYPE")]
    pub output: Option<String>,

    /// Divider string for text-divider plugin I/O.
    #[arg(long, value_name = "STRING")]
    pub divider: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct EditArgs {
    /// Query selecting the action to edit.
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,

    /// Replace action text directly (non-interactive). Without this flag,
    /// matched actions are opened in `$EDITOR` for multi-action editing.
    #[arg(long = "text", value_name = "TEXT")]
    pub text: Option<String>,

    /// Restrict edits to a specific file path.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Search for todo files this many levels deep.
    #[arg(short = 'd', long, default_value_t = 1)]
    pub depth: usize,

    /// Restrict edits to known todo files matching tokens.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Additional search terms used as a fallback filter.
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,

    /// Match actions containing tag expressions.
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Include completed (`@done`) actions.
    #[arg(long = "done", default_value_t = false)]
    pub done: bool,

    /// Treat search as regular expression.
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,

    /// Exact phrase match.
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,

    /// Include notes while searching (default: true).
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,

    /// Exclude notes while searching.
    #[arg(long = "no-search-notes", default_value_t = false)]
    pub no_search_notes: bool,

    /// Editor override (default: `$EDITOR` / `$GIT_EDITOR`).
    #[arg(long = "editor", value_name = "EDITOR")]
    pub editor: Option<String>,

    /// Edit all matched actions.
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ArchiveArgs {
    /// Optional action query.
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,

    /// Archive all already-done tasks.
    #[arg(long, default_value_t = false)]
    pub done: bool,

    /// Restrict actions to a specific file path.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Search for todo files this many levels deep.
    #[arg(short = 'd', long, default_value_t = 1)]
    pub depth: usize,

    /// Restrict by tag(s).
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Restrict by project.
    #[arg(long = "project", visible_alias = "proj", value_name = "PROJECT")]
    pub project: Option<String>,

    /// Restrict to known todo files matching tokens.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Additional search terms used as fallback filter.
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,

    /// Interpret query as regex (reserved parity flag).
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,

    /// Interpret query as exact (reserved parity flag).
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,

    /// Act on all matches without menu selection.
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Append note(s) to archived actions.
    #[arg(short = 'n', long = "note", value_name = "NOTE", num_args = 0..)]
    pub note: Vec<String>,

    /// Replace existing notes instead of appending.
    #[arg(short = 'o', long = "overwrite", default_value_t = false)]
    pub overwrite_notes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct CompletedArgs {
    /// Optional pattern tokens to match completed action text.
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub pattern: Vec<String>,

    /// Display actions completed before date/time.
    #[arg(short = 'b', long = "before", value_name = "DATE")]
    pub before: Option<String>,

    /// Display actions completed on date.
    #[arg(long = "on", value_name = "DATE")]
    pub on: Option<String>,

    /// Display actions completed after date/time.
    #[arg(short = 'a', long = "after", value_name = "DATE")]
    pub after: Option<String>,

    /// Combine before/on/after ranges with OR semantics.
    #[arg(short = 'o', long = "or", default_value_t = false)]
    pub or_mode: bool,

    /// Recurse to depth when searching for files.
    #[arg(short = 'd', long = "depth")]
    pub depth: Option<usize>,

    /// Display matches from known todo files in history.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,

    /// Include notes in output.
    #[arg(long, default_value_t = false)]
    pub notes: bool,

    /// Omit notes from output (pairs with --notes).
    #[arg(long = "no-notes", default_value_t = false)]
    pub no_notes: bool,

    /// Include notes while searching.
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,

    /// Exclude notes while searching.
    #[arg(long = "no-search-notes", default_value_t = false)]
    pub no_search_notes: bool,

    /// Restrict by project name.
    #[arg(long = "project", visible_alias = "proj", value_name = "PROJECT")]
    pub project: Option<String>,

    /// Restrict by tag(s) presence.
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,

    /// Group output by todo file.
    #[arg(long, default_value_t = false)]
    pub nest: bool,

    /// Nested by file and project.
    #[arg(long, default_value_t = false)]
    pub omnifocus: bool,

    /// Omit filename in output (same semantics as `find --no-file`).
    #[arg(long = "no-file", default_value_t = false)]
    pub no_file: bool,

    /// Save this completed query.
    #[arg(long, value_name = "TITLE")]
    pub save: Option<String>,
}

impl CompletedArgs {
    pub fn effective_search_notes(&self) -> bool {
        self.search_notes && !self.no_search_notes
    }

    pub fn effective_notes(&self) -> bool {
        self.notes && !self.no_notes
    }

    pub fn nest_for_display(&self) -> bool {
        self.nest || self.omnifocus
    }
}

#[derive(Debug, Args)]
pub struct SavedArgs {
    #[command(subcommand)]
    pub command: SavedCommands,
}

#[derive(Debug, Subcommand)]
pub enum SavedCommands {
    /// List saved search titles.
    List,
    /// Run a saved search.
    Run {
        #[arg(value_name = "TITLE")]
        title: String,
    },
    /// Edit a saved search definition.
    Edit {
        #[arg(value_name = "TITLE")]
        title: String,
        /// Optional editor binary override.
        #[arg(long, value_name = "EDITOR")]
        editor: Option<String>,
    },
    /// Delete a saved search.
    Delete {
        #[arg(value_name = "TITLE")]
        title: String,
    },
    /// Select from saved searches interactively.
    Select,
}

#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub command: PluginCommands,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommands {
    /// List discovered plugins.
    List,
    /// Run plugin against selected actions.
    Run {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
        /// Optional query filter (Ruby `plugin run` uses `--search` / `--tagged` instead).
        #[arg(value_name = "QUERY", num_args = 0..=1)]
        query: Option<String>,
        #[arg(long, value_name = "TYPE")]
        input: Option<String>,
        #[arg(long, value_name = "TYPE")]
        output: Option<String>,
        #[arg(long, value_name = "STRING")]
        divider: Option<String>,
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        #[arg(short = 'd', long = "depth")]
        depth: Option<usize>,
        #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
        in_todo: Vec<String>,
        #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
        search: Vec<String>,
        #[arg(long, default_value_t = false)]
        done: bool,
        #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
        tagged: Vec<String>,
    },
    /// Enable a plugin by setting executable permissions.
    Enable {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
    },
    /// Disable a plugin by removing executable permissions.
    Disable {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
    },
    /// Create a new plugin file in the plugins directory.
    New {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
    },
    /// Open a plugin file in an editor.
    Edit {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
        #[arg(long, value_name = "EDITOR")]
        editor: Option<String>,
    },
    /// Generate example plugin fixtures.
    GenerateExamples,
}

#[derive(Debug, Args, Clone)]
pub struct MoveArgs {
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,
    #[arg(long = "to", visible_alias = "move", value_name = "PROJECT")]
    pub to: String,
    #[arg(long = "at", value_name = "POSITION")]
    pub at: Option<String>,
    #[arg(long = "from", value_name = "PROJECT")]
    pub from: Option<String>,
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,
    #[arg(long = "file", value_name = "PATH")]
    pub file: Option<PathBuf>,
    #[arg(short = 'd', long = "depth", default_value_t = 1)]
    pub depth: usize,
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

#[derive(Debug, Args, Clone)]
pub struct TagArgs {
    #[arg(value_name = "TAG", num_args = 0..)]
    pub tags: Vec<String>,
    #[arg(value_name = "QUERY")]
    pub query: Option<String>,
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,
    #[arg(long = "done", default_value_t = false)]
    pub done: bool,
    #[arg(long = "file", value_name = "PATH")]
    pub file: Option<PathBuf>,
    #[arg(short = 'd', long = "depth", default_value_t = 1)]
    pub depth: usize,
    #[arg(long = "tagged", value_name = "TAG", num_args = 0..)]
    pub tagged: Vec<String>,
    #[arg(short = 'e', long = "regex", default_value_t = false)]
    pub regex: bool,
    #[arg(short = 'x', long = "exact", default_value_t = false)]
    pub exact: bool,
    #[arg(long = "search", visible_aliases = ["find", "grep"], value_name = "QUERY", num_args = 0..)]
    pub search: Vec<String>,
    #[arg(long = "search-notes", default_value_t = true)]
    pub search_notes: bool,
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

#[derive(Debug, Args, Clone)]
pub struct OpenArgs {
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,
    #[arg(short = 'd', long = "depth", default_value_t = 1)]
    pub depth: usize,
    #[arg(short = 'e', long = "editor", value_name = "EDITOR")]
    pub editor: Option<String>,
    #[arg(short = 'a', long = "app", value_name = "APP")]
    pub app: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct ProjectsArgs {
    #[arg(short = 'd', long = "depth", default_value_t = 1)]
    pub depth: usize,
    #[arg(short = 'p', long = "paths", default_value_t = false)]
    pub paths: bool,
}

#[derive(Debug, Args, Clone)]
pub struct TodosArgs {
    /// Fuzzy-match known todo files (path tokens). Separate with `/`, `:`, or spaces.
    #[arg(value_name = "QUERY", num_args = 0..)]
    pub query: Vec<String>,

    /// Open the known-todo database (`tdlist.txt`) in `$EDITOR`.
    #[arg(short = 'e', long = "edit", default_value_t = false)]
    pub edit: bool,
}

#[derive(Debug, Args, Clone)]
pub struct UndoArgs {
    #[arg(
        short = 's',
        long = "select",
        visible_alias = "choose",
        default_value_t = false
    )]
    pub select: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ScanArgs {
    #[arg(short = 'd', long = "depth", default_value_t = 3)]
    pub depth: usize,
    #[arg(short = 'p', long = "prune", default_value_t = false)]
    pub prune: bool,
    #[arg(long = "hidden", default_value_t = false)]
    pub hidden: bool,
    #[arg(short = 'n', long = "dry-run", default_value_t = false)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct PromptArgs {
    #[command(subcommand)]
    pub command: Option<PromptCommands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PromptCommands {
    Show,
    Install,
}

const KNOWN_COMMANDS: &[&str] = &[
    "next", "show", "find", "grep", "search", "tagged", "add", "update", "edit", "complete",
    "finish", "archive", "completed", "finished", "restore", "unfinish", "move", "tag", "open",
    "projects", "todos", "undo", "scan", "init", "create", "prompt", "changes", "changelog",
    "saved", "initconfig", "init-config", "plugin", "help", "version",
];

const SAVED_SUBCOMMANDS: &[&str] = &["list", "run", "edit", "delete", "select"];

/// Parse CLI args with Ruby-compatible rewrites (saved search dispatch, legacy `-a` add shim).
pub fn parse_cli() -> Cli {
    parse_from_argv(std::env::args())
}

pub fn parse_from_argv(args: impl IntoIterator<Item = impl AsRef<str>>) -> Cli {
    let args: Vec<String> = args
        .into_iter()
        .map(|a| a.as_ref().to_string())
        .collect();
    Cli::parse_from(normalize_argv(&args))
}

/// Note/priority globals for deprecated `na --add` / `na -a` (not stored on [`Cli`] to avoid clap conflicts).
pub fn legacy_global_add_extras_from_argv() -> (bool, Option<String>) {
    let args: Vec<String> = std::env::args().collect();
    let (globals, _) = split_globals_rest(&args);
    let note = globals
        .iter()
        .any(|g| g == "--note" || g == "-n");
    let mut priority = None;
    for (idx, g) in globals.iter().enumerate() {
        if g == "--priority" || g == "-p" {
            if let Some(val) = globals.get(idx + 1) {
                if !val.starts_with('-') {
                    priority = Some(val.clone());
                }
            }
        }
    }
    (note, priority)
}

fn normalize_argv(args: &[String]) -> Vec<String> {
    let mut out = expand_legacy_global_short_flags(args);
    out = rewrite_saved_positional(&out);
    out = rewrite_unknown_single_command(&out);
    out = rewrite_global_add_shim(&out);
    out
}

/// Map deprecated global short flags to long forms clap can parse without subcommand conflicts.
fn expand_legacy_global_short_flags(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        return args.to_vec();
    }
    let mut out = vec![args[0].clone()];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-a" => {
                out.push("--add".into());
                i += 1;
            }
            "-r" => {
                out.push("--recurse".into());
                i += 1;
            }
            "-n" => {
                out.push("--note".into());
                i += 1;
            }
            "-p" => {
                out.push("--priority".into());
                i += 1;
                if i < args.len() && !args[i].starts_with('-') {
                    out.push(args[i].clone());
                    i += 1;
                }
            }
            arg if arg.starts_with('-') => {
                out.push(args[i].clone());
                i += 1;
            }
            _ => {
                out.extend_from_slice(&args[i..]);
                break;
            }
        }
    }
    out
}

fn split_globals_rest(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut globals = Vec::new();
    let mut rest = Vec::new();
    let mut in_globals = true;
    for arg in args.iter().skip(1) {
        if in_globals && arg.starts_with('-') {
            globals.push(arg.clone());
        } else {
            in_globals = false;
            rest.push(arg.clone());
        }
    }
    (globals, rest)
}

fn is_known_command(name: &str) -> bool {
    KNOWN_COMMANDS
        .iter()
        .any(|cmd| cmd.eq_ignore_ascii_case(name))
}

fn rewrite_saved_positional(args: &[String]) -> Vec<String> {
    let (globals, rest) = split_globals_rest(args);
    if rest.first().map(|s| s.as_str()) != Some("saved") {
        return args.to_vec();
    }
    if rest.len() >= 2 && !SAVED_SUBCOMMANDS.contains(&rest[1].as_str()) {
        let mut new_args = vec![args[0].clone()];
        new_args.extend(globals);
        new_args.push("saved".into());
        new_args.push("run".into());
        new_args.extend(rest.into_iter().skip(1));
        return new_args;
    }
    args.to_vec()
}

fn rewrite_unknown_single_command(args: &[String]) -> Vec<String> {
    let (globals, rest) = split_globals_rest(args);
    if rest.len() != 1 || is_known_command(&rest[0]) {
        return args.to_vec();
    }
    let mut new_args = vec![args[0].clone()];
    new_args.extend(globals);
    new_args.push("saved".into());
    new_args.push("run".into());
    new_args.extend(rest);
    new_args
}

fn rewrite_global_add_shim(args: &[String]) -> Vec<String> {
    let (globals, rest) = split_globals_rest(args);
    let has_add = globals.iter().any(|g| g == "-a" || g == "--add");
    if !has_add || rest.is_empty() || is_known_command(&rest[0]) {
        return args.to_vec();
    }
    if rest.len() == 1 {
        return args.to_vec();
    }
    let mut new_args = vec![args[0].clone()];
    new_args.extend(
        globals
            .into_iter()
            .filter(|g| g != "-a" && g != "--add"),
    );
    new_args.push("add".into());
    new_args.push(rest.join(" "));
    new_args
}

#[cfg(test)]
mod tests {
    use super::{parse_from_argv, Cli, Commands};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn next_available_alias_sets_first_available() {
        let cli = Cli::parse_from(["na", "next", "--available"]);
        match cli.command {
            Some(Commands::Next(args)) => assert!(args.first_available),
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn next_short_a_sets_first_available() {
        let cli = Cli::parse_from(["na", "next", "-a"]);
        match cli.command {
            Some(Commands::Next(args)) => assert!(args.first_available),
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn global_ext_alias_parses() {
        let cli = Cli::parse_from(["na", "--ext", "tp", "next"]);
        assert_eq!(cli.extension.as_str(), "tp");
        assert!(matches!(cli.command, Some(Commands::Next(_))));
    }

    #[test]
    fn next_file_and_depth_flags_parse() {
        let cli = Cli::parse_from(["na", "next", "--file", "x.taskpaper", "--depth", "3"]);
        match cli.command {
            Some(Commands::Next(args)) => {
                assert_eq!(args.file, Some(PathBuf::from("x.taskpaper")));
                assert_eq!(args.depth, Some(3));
            }
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn find_command_alias_search_parses() {
        let cli = Cli::parse_from(["na", "search", "@home"]);
        match cli.command {
            Some(Commands::Find(args)) => assert_eq!(args.query, "@home"),
            _ => panic!("expected find command alias"),
        }
    }

    #[test]
    fn version_flag_parses() {
        let cli = Cli::parse_from(["na", "--version"]);
        assert!(cli.version);
    }

    #[test]
    fn next_no_search_notes_parses() {
        let cli = Cli::parse_from(["na", "next", "--no-search-notes"]);
        match cli.command {
            Some(Commands::Next(args)) => {
                assert!(args.no_search_notes);
            }
            _ => panic!("expected next"),
        }
    }

    #[test]
    fn next_no_notes_parses() {
        let cli = Cli::parse_from(["na", "next", "--notes", "--no-notes"]);
        match cli.command {
            Some(Commands::Next(args)) => assert!(args.no_notes),
            _ => panic!("expected next"),
        }
    }

    #[test]
    fn add_flags_parse() {
        let cli = Cli::parse_from([
            "na",
            "add",
            "Ship it",
            "--started",
            "2026-04-20 09:00",
            "--end",
            "2026-04-20 10:00",
            "--duration",
            "1h",
            "--to",
            "Work",
            "--at",
            "start",
            "--in",
            "project",
            "-p",
            "h",
            "-t",
            "next",
            "-x",
            "-f",
            "tasks.taskpaper",
            "--finish",
            "-d",
            "3",
            "-n",
        ]);
        match cli.command {
            Some(Commands::Add(args)) => {
                assert_eq!(args.started.as_deref(), Some("2026-04-20 09:00"));
                assert_eq!(args.end.as_deref(), Some("2026-04-20 10:00"));
                assert_eq!(args.duration.as_deref(), Some("1h"));
                assert_eq!(args.project, "Work");
                assert_eq!(args.at.as_deref(), Some("start"));
                assert_eq!(args.in_todo, vec!["project".to_string()]);
                assert_eq!(args.priority.as_deref(), Some("h"));
                assert_eq!(args.tag.as_deref(), Some("next"));
                assert!(args.no_next_tag);
                assert_eq!(args.file, Some(PathBuf::from("tasks.taskpaper")));
                assert!(args.finish);
                assert_eq!(args.depth, Some(3));
                assert!(args.note);
            }
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn update_flags_parse() {
        let cli = Cli::parse_from([
            "na",
            "update",
            "--tag",
            "@home",
            "--remove",
            "@na",
            "--replace",
            "new text",
            "--to",
            "Inbox",
            "--note",
            "one",
            "--overwrite-notes",
            "--restore",
            "--finish",
            "--file",
            "tasks.taskpaper",
            "--in",
            "work",
            "--search",
            "needle",
            "--all",
            "-d",
            "3",
            "task text",
        ]);
        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.query.as_deref(), Some("task text"));
                assert_eq!(args.tag, vec!["@home".to_string()]);
                assert_eq!(args.untag, vec!["@na".to_string()]);
                assert!(args.done);
                assert_eq!(args.replace.as_deref(), Some("new text"));
                assert_eq!(args.to.as_deref(), Some("Inbox"));
                assert_eq!(args.note, vec!["one".to_string()]);
                assert!(args.overwrite_notes);
                assert!(args.restore);
                assert_eq!(args.file, Some(PathBuf::from("tasks.taskpaper")));
                assert_eq!(args.in_todo, vec!["work".to_string()]);
                assert_eq!(args.search, vec!["needle".to_string()]);
                assert!(args.all);
                assert_eq!(args.depth, 3);
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn update_short_r_f_o_parse() {
        let cli = Cli::parse_from(["na", "update", "-r", "@na", "-f", "-o", "--all", "needle"]);
        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.untag, vec!["@na".to_string()]);
                assert!(args.done);
                assert!(args.overwrite_notes);
                assert!(args.all);
                assert_eq!(args.query.as_deref(), Some("needle"));
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn update_overwrite_visible_alias_parses() {
        let cli = Cli::parse_from(["na", "update", "--overwrite", "--all", "q"]);
        match cli.command {
            Some(Commands::Update(args)) => {
                assert!(args.overwrite_notes);
                assert!(args.all);
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn move_to_visible_alias_move_parses() {
        let cli = Cli::parse_from(["na", "move", "find me", "--move", "Bugs"]);
        match cli.command {
            Some(Commands::Move(args)) => {
                assert_eq!(args.query.as_deref(), Some("find me"));
                assert_eq!(args.to.as_str(), "Bugs");
            }
            _ => panic!("expected move"),
        }
    }

    #[test]
    fn update_editor_flag_parses() {
        let cli = Cli::parse_from([
            "na", "update", "--editor", "nano", "--edit", "--all", "needle",
        ]);
        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.editor.as_deref(), Some("nano"));
                assert!(args.edit);
                assert!(args.all);
                assert_eq!(args.query.as_deref(), Some("needle"));
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn edit_flags_parse() {
        let cli = Cli::parse_from([
            "na",
            "edit",
            "--text",
            "new body",
            "--file",
            "tasks.taskpaper",
            "--in",
            "work",
            "--search",
            "needle",
            "--all",
            "-d",
            "3",
            "task text",
        ]);
        match cli.command {
            Some(Commands::Edit(args)) => {
                assert_eq!(args.query.as_deref(), Some("task text"));
                assert_eq!(args.text.as_deref(), Some("new body"));
                assert_eq!(args.file, Some(PathBuf::from("tasks.taskpaper")));
                assert_eq!(args.in_todo, vec!["work".to_string()]);
                assert_eq!(args.search, vec!["needle".to_string()]);
                assert!(args.all);
                assert_eq!(args.depth, 3);
            }
            _ => panic!("expected edit command"),
        }
    }

    #[test]
    fn complete_finish_alias_parses() {
        let cli = Cli::parse_from(["na", "finish", "task text"]);
        match cli.command {
            Some(Commands::Complete(args)) => assert_eq!(args.query.as_deref(), Some("task text")),
            _ => panic!("expected complete via finish alias"),
        }
    }

    #[test]
    fn archive_flags_parse() {
        let cli = Cli::parse_from([
            "na",
            "archive",
            "--done",
            "--file",
            "tasks.taskpaper",
            "--depth",
            "3",
            "--tagged",
            "home",
            "--project",
            "Work",
            "--in",
            "work",
            "--search",
            "token",
            "--regex",
            "--exact",
            "--all",
            "task text",
        ]);
        match cli.command {
            Some(Commands::Archive(args)) => {
                assert_eq!(args.query.as_deref(), Some("task text"));
                assert!(args.done);
                assert_eq!(args.file, Some(PathBuf::from("tasks.taskpaper")));
                assert_eq!(args.depth, 3);
                assert_eq!(args.tagged, vec!["home".to_string()]);
                assert_eq!(args.project.as_deref(), Some("Work"));
                assert_eq!(args.in_todo, vec!["work".to_string()]);
                assert_eq!(args.search, vec!["token".to_string()]);
                assert!(args.regex);
                assert!(args.exact);
                assert!(args.all);
            }
            _ => panic!("expected archive command"),
        }
    }

    #[test]
    fn completed_finished_alias_parses() {
        let cli = Cli::parse_from(["na", "finished", "--before", "2026-04-20", "feature"]);
        match cli.command {
            Some(Commands::Completed(args)) => {
                assert_eq!(args.before.as_deref(), Some("2026-04-20"));
                assert_eq!(args.pattern, vec!["feature".to_string()]);
            }
            _ => panic!("expected completed via finished alias"),
        }
    }

    #[test]
    fn completed_no_file_parses() {
        let cli = Cli::parse_from(["na", "completed", "--no-file"]);
        match cli.command {
            Some(Commands::Completed(args)) => assert!(args.no_file),
            _ => panic!("expected completed"),
        }
    }

    #[test]
    fn saved_run_subcommand_parses() {
        let cli = Cli::parse_from(["na", "saved", "run", "Weekly Focus"]);
        match cli.command {
            Some(Commands::Saved(args)) => match args.command {
                super::SavedCommands::Run { title } => assert_eq!(title, "Weekly Focus"),
                _ => panic!("expected saved run"),
            },
            _ => panic!("expected saved"),
        }
    }

    #[test]
    fn saved_edit_editor_override_parses() {
        let cli = Cli::parse_from(["na", "saved", "edit", "Weekly Focus", "--editor", "nano"]);
        match cli.command {
            Some(Commands::Saved(args)) => match args.command {
                super::SavedCommands::Edit { title, editor } => {
                    assert_eq!(title, "Weekly Focus");
                    assert_eq!(editor.as_deref(), Some("nano"));
                }
                _ => panic!("expected saved edit"),
            },
            _ => panic!("expected saved"),
        }
    }

    #[test]
    fn update_plugin_flags_parse() {
        let cli = Cli::parse_from([
            "na",
            "update",
            "--plugin",
            "fmt",
            "--input",
            "json",
            "--output",
            "yaml",
            "--divider",
            "||",
            "--all",
            "needle",
        ]);
        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.plugin.as_deref(), Some("fmt"));
                assert_eq!(args.input.as_deref(), Some("json"));
                assert_eq!(args.output.as_deref(), Some("yaml"));
                assert_eq!(args.divider.as_deref(), Some("||"));
                assert!(args.all);
                assert_eq!(args.query.as_deref(), Some("needle"));
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn initconfig_force_flag_parses() {
        let cli = Cli::parse_from(["na", "initconfig", "--force"]);
        match cli.command {
            Some(Commands::InitConfig(args)) => assert!(args.force),
            _ => panic!("expected initconfig"),
        }
    }

    #[test]
    fn global_na_tag_and_add_at_parse() {
        let cli = Cli::parse_from([
            "na",
            "--na_tag",
            "next",
            "--add_at",
            "end",
            "--depth",
            "2",
            "next",
        ]);
        assert_eq!(cli.na_tag, "next");
        assert_eq!(cli.add_at, "end");
        assert_eq!(cli.depth, Some(2));
    }

    #[test]
    fn plugin_management_subcommands_parse() {
        let enable = Cli::parse_from(["na", "plugin", "enable", "fmt"]);
        match enable.command {
            Some(Commands::Plugin(args)) => match args.command {
                super::PluginCommands::Enable { plugin } => assert_eq!(plugin, "fmt"),
                _ => panic!("expected plugin enable"),
            },
            _ => panic!("expected plugin"),
        }

        let disable = Cli::parse_from(["na", "plugin", "disable", "fmt"]);
        match disable.command {
            Some(Commands::Plugin(args)) => match args.command {
                super::PluginCommands::Disable { plugin } => assert_eq!(plugin, "fmt"),
                _ => panic!("expected plugin disable"),
            },
            _ => panic!("expected plugin"),
        }

        let new_cmd = Cli::parse_from(["na", "plugin", "new", "fmt"]);
        match new_cmd.command {
            Some(Commands::Plugin(args)) => match args.command {
                super::PluginCommands::New { plugin } => assert_eq!(plugin, "fmt"),
                _ => panic!("expected plugin new"),
            },
            _ => panic!("expected plugin"),
        }
    }

    #[test]
    fn unknown_single_arg_dispatches_to_saved_run() {
        let cli = parse_from_argv(["na", "Weekly Focus"]);
        match cli.command {
            Some(Commands::Saved(args)) => match args.command {
                super::SavedCommands::Run { title } => assert_eq!(title, "Weekly Focus"),
                _ => panic!("expected saved run"),
            },
            _ => panic!("expected saved"),
        }
    }

    #[test]
    fn saved_positional_title_rewrites_to_run() {
        let cli = parse_from_argv(["na", "saved", "Weekly Focus"]);
        match cli.command {
            Some(Commands::Saved(args)) => match args.command {
                super::SavedCommands::Run { title } => assert_eq!(title, "Weekly Focus"),
                _ => panic!("expected saved run"),
            },
            _ => panic!("expected saved"),
        }
    }

    #[test]
    fn legacy_global_add_shim_rewrites_to_add() {
        let cli = parse_from_argv(["na", "-a", "Buy", "milk"]);
        match cli.command {
            Some(Commands::Add(args)) => {
                assert_eq!(args.text, "Buy milk");
            }
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn include_ext_flag_parses() {
        let cli = Cli::parse_from(["na", "next", "--include_ext"]);
        assert!(cli.include_ext);
    }

    #[test]
    fn color_flag_parses() {
        let cli = Cli::parse_from(["na", "next", "--color"]);
        assert!(cli.color);
    }

    #[test]
    fn pager_and_no_pager_flags_parse() {
        let cli = Cli::parse_from(["na", "next", "--no-pager"]);
        assert!(!cli.pager || cli.no_pager);
        assert!(cli.no_pager);
    }

    #[test]
    fn repo_top_flag_parses() {
        let cli = Cli::parse_from(["na", "--repo-top", "next"]);
        assert!(cli.repo_top);
    }

    #[test]
    fn legacy_global_short_flags_expand_for_parse() {
        let cli = parse_from_argv(["na", "-r", "--add", "next", "Ship"]);
        assert!(cli.recurse);
        assert!(cli.add);
        match cli.command {
            Some(Commands::Next(args)) => assert_eq!(args.filter.as_deref(), Some("Ship")),
            _ => panic!("expected next"),
        }
    }

    #[test]
    fn legacy_recurse_short_flag_expands() {
        let cli = parse_from_argv(["na", "-r", "find"]);
        assert!(cli.recurse);
    }

    #[test]
    fn next_available_short_a_parses_on_subcommand() {
        let cli = Cli::parse_from(["na", "next", "-a"]);
        assert!(!cli.add);
        match cli.command {
            Some(Commands::Next(args)) => assert!(args.first_available),
            _ => panic!("expected next"),
        }
    }

    #[test]
    fn debug_flag_parses() {
        let cli = Cli::parse_from(["na", "--debug", "find"]);
        assert!(cli.debug);
    }
}
