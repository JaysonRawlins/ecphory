# subject-index Specification

## Purpose
Give every harness the recall keys, not just the one that ships its own memory
file. Claude Code injects a per-project `MEMORY.md` of search keys, and holding
that index is what makes recall happen — an agent without it can still reach the
store but never learns which questions to ask. Codex, agy and OpenCode read
`AGENTS.md` and get no index at all. A workspace's keys are rendered into both
files from the store, keys only, inside a marked region: the file stays a
pointer and ecphory keeps the content.

## Requirements
### Requirement: A workspace key both agree on
The system SHALL derive a workspace key from a directory as the slug of its canonical working-copy root: `git rev-parse --path-format=absolute --git-common-dir` with the trailing `.git` removed when the common dir is an ordinary `<root>/.git`, falling back to `--show-toplevel`, and to the directory itself when it is not in a repository. The slug SHALL match Claude Code's `sanitizePath`: every UTF-16 code unit outside `[A-Za-z0-9]` becomes `-`, and a result longer than 200 characters is truncated to 200 and suffixed with `-` plus the base36 rendering of `Math.abs` of the 32-bit `h = h*31 + unit` hash of the original path. The key is exposed as `ecphory workspace-key` and used as the tag `ws:<slug>`.

#### Scenario: A linked worktree is the same workspace
- **WHEN** `ecphory workspace-key` runs in a linked worktree and again in that repository's main checkout
- **THEN** both print the same `ws:<slug>`, and that slug is the one Claude Code uses for the repository's memory directory

#### Scenario: No repository
- **WHEN** the directory is not inside a git repository
- **THEN** the key is the slug of that directory itself

### Requirement: The rendered index carries keys, never bodies
A rendered index SHALL contain one row per non-deleted episode carrying the workspace's `ws:` tag, and each row SHALL carry only: the episode's first non-empty `search_phrases` entry as the query and its name as the description, or — when it has no search phrase — its name as the query and an empty description; its remaining tags, the workspace tag itself excluded; and its id. Episode `content` SHALL NOT appear in the rendered file. An episode with neither a name nor a search phrase SHALL be omitted. Cell values are flattened to a single line with `|` escaped.

#### Scenario: One workspace's episodes only
- **WHEN** one episode is tagged `ws:X` and another `ws:Y`, and the index for `X` is rendered
- **THEN** the file gains exactly one row, for the `ws:X` episode

#### Scenario: No body text
- **WHEN** an episode's content contains a distinctive string
- **THEN** that string appears nowhere in the rendered file

### Requirement: Rendering is idempotent inside a marked region
The rendered block SHALL be delimited by `<!-- BEGIN ecphory subject index -->` and `<!-- END ecphory subject index -->`, SHALL order rows by episode id descending (newest first), SHALL be deterministic for a given set of episodes (no timestamps or counters), and SHALL replace only the region — every byte outside the markers is preserved. A file with exactly one marker, or with `END` before `BEGIN`, SHALL be an error and nothing SHALL be written. The file SHALL be written only when its bytes change, through a temp file and a rename; a symlinked target SHALL be followed rather than replaced.

#### Scenario: A symlinked target
- **WHEN** the target path is a symlink to another file and the index is rendered
- **THEN** the link survives and the file it points at carries the index

#### Scenario: Re-render with no new episodes
- **WHEN** the render runs twice against the same episodes
- **THEN** the second run reports the target unchanged and the file is byte-identical

#### Scenario: Hand-written content survives
- **WHEN** the target file begins with hand-written prose and the index is rendered, twice, with different episodes
- **THEN** the prose is unchanged both times and the file holds exactly one marked region

### Requirement: Default render targets
With no `--out`, the render SHALL write `<workspace>/AGENTS.md` and `<CLAUDE_CONFIG_DIR or ~/.claude>/projects/<slug>/memory/MEMORY.md`. Any `--out` path given SHALL replace the default set entirely.

#### Scenario: Both harness files
- **WHEN** `ecphory render-index` runs with no `--out`
- **THEN** the repo-root `AGENTS.md` and the Claude memory `MEMORY.md` for that workspace's slug both carry the index

### Requirement: The render reads the daemon, not the database
`render-index` and `workspace-key` SHALL NOT open the store. `render-index` SHALL read the episodes over HTTP from the daemon at `--url`, and SHALL fail loudly rather than silently truncate if the listing reaches its cap.

#### Scenario: Rendering while the daemon holds the lock
- **WHEN** the daemon is serving a store and `ecphory render-index` runs against it
- **THEN** the render succeeds, while a command that opens the same store directly fails with `Database already open. Cannot acquire lock.`

### Requirement: Listings can be scoped by tag
`GET /api/v1/memory/episodes` SHALL accept `tags` as a comma-separated list and return only episodes carrying all of them, applying the filter before `max_results`.

#### Scenario: Scoped listing
- **WHEN** two episodes carry different `ws:` tags and the listing names one of them
- **THEN** only that episode is returned

### Requirement: Staleness is checkable without writing
`render-index --check` SHALL write nothing, SHALL print each stale target, and SHALL exit non-zero when any target differs from what a render would produce.

#### Scenario: A new episode makes the index stale
- **WHEN** `--check` runs immediately after a render, and again after an episode joins the workspace
- **THEN** the first run exits 0 and the second exits 1 naming the stale target

