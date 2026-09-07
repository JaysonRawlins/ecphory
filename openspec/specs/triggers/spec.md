# triggers Specification

## Purpose
Regenerate an artifact *derived* from an episode (a rendered context file, an exported
doc) on the write itself, rather than relying on each client to remember a second manual
step. The store is the one layer every client shares, so a hook there covers MCP, REST,
CLI, and clients that do not exist yet. Triggers are observers: they never fail, delay,
or roll back the write that fired them.

## Requirements
### Requirement: A matching episode write runs the operator's command
The system SHALL read `ECPHORY_TRIGGERS_FILE` (a JSON config) when the store is opened,
and after a successful episode mutation SHALL run the configured `run` argv for every
trigger whose event subscription and matcher both match. Because the engine is attached
where the store is opened, every entry point — MCP, REST and CLI — fires the same
triggers. An unset `ECPHORY_TRIGGERS_FILE` SHALL leave the feature off.

#### Scenario: Tagged episode is updated
- **WHEN** a trigger matches `tags_any: ["rendered-artifact"]` and an episode carrying that tag is updated
- **THEN** the configured command runs

#### Scenario: Non-matching episode
- **WHEN** an episode without any matching tag or id prefix is written
- **THEN** no command runs

#### Scenario: Feature off
- **WHEN** `ECPHORY_TRIGGERS_FILE` is unset
- **THEN** no triggers load and writes behave exactly as they did before this capability

### Requirement: The event vocabulary is fixed and the default set is narrow
The system SHALL accept only the known events `insert`, `update`, `demote`, `restore`,
`restore_version` (episode events) and `export` (store-wide). A trigger that omits
`events` SHALL subscribe to `update`, `restore` and `restore_version` — the three that
change an existing episode's content, which is the drift class this capability closes.
`insert` and `demote` SHALL be opt-in, and store-wide events SHALL never be in the
default set. Store-wide event behaviour is specified in the `offsite-mirror` capability.

#### Scenario: Default subscription
- **WHEN** a trigger omits `events` and an episode it matches is inserted
- **THEN** the command does not run, because `insert` is not in the default set

#### Scenario: Unknown event
- **WHEN** a config names an event outside the known set
- **THEN** the config is rejected at load with an error naming the trigger and the event

### Requirement: A trigger can never compromise the write that fired it
Trigger execution SHALL be fire-and-forget: the system SHALL spawn the command and
return immediately, so a slow, failing, or unspawnable command SHALL NOT fail, delay, or
roll back the write. A command that exceeds `timeout_seconds` (default 60) SHALL be
killed. Every failure mode SHALL be logged at warn and SHALL be the only consequence.

#### Scenario: Command exits non-zero
- **WHEN** a triggered command fails
- **THEN** the write that fired it still succeeds and the failure is logged at warn

#### Scenario: Command hangs
- **WHEN** a triggered command outlives `timeout_seconds`
- **THEN** it is killed and the timeout is logged, and the write is unaffected

### Requirement: Runs of one trigger serialize
The system SHALL serialize overlapping runs of the same trigger, so a second fire queues
behind the first rather than racing it — renders are idempotent, but two concurrent
writers to one output file are not. Different triggers SHALL be free to run in parallel.

#### Scenario: Two writes in quick succession
- **WHEN** two matching writes fire the same trigger before the first run finishes
- **THEN** the second run waits for the first to complete rather than running concurrently

### Requirement: A config that could misfire is rejected at load
The system SHALL reject at load: an empty `run`; a `run[0]` that is not an absolute
path; an unknown event name; and an empty `match` on a trigger subscribed to any episode
event. An empty matcher SHALL NOT mean match-all — a command running on every write in
the store is never what an operator intended.

#### Scenario: Empty matcher on an episode event
- **WHEN** a trigger subscribes to `update` and sets no `tags_any` or `id_prefix`
- **THEN** the config is rejected with an error requiring `tags_any` and/or `id_prefix`

#### Scenario: Relative program path
- **WHEN** `run[0]` is a relative path
- **THEN** the config is rejected, so what executes never depends on the process working directory or `PATH`

### Requirement: A malformed config disables triggers loudly, and the store still serves
A present-but-unreadable or invalid `ECPHORY_TRIGGERS_FILE` SHALL log at ERROR and
disable the feature, and the store SHALL still open and serve. Memory availability
outranks a derived artifact, and a crash-looping service would take every agent with it.
The failure SHALL NOT be silent.

#### Scenario: Invalid JSON in the config
- **WHEN** the triggers file is malformed and the store is opened
- **THEN** an ERROR naming the file is logged, no triggers are active, and search and writes work normally

### Requirement: The command learns which trigger and event fired it
The system SHALL pass `ECPHORY_TRIGGER_NAME` and `ECPHORY_TRIGGER_EVENT` in the
command's environment, plus `ECPHORY_TRIGGER_EPISODE_ID` on episode events. The command's
stdin SHALL be null and its stdout discarded, so a trigger cannot block on input or
pollute the service's output; stderr SHALL be captured for the log.

#### Scenario: Render script needs the episode
- **WHEN** an episode event fires a trigger
- **THEN** the command's environment carries the trigger name, the event name, and the episode id

### Requirement: Episode content never determines what runs
The set of commands the system can run SHALL come only from the operator-owned local
config file. Episode content, tags and metadata SHALL influence only *whether* a
configured command runs, never *what* runs.

#### Scenario: Episode text cannot inject a command
- **WHEN** an episode's content or tags contain anything resembling a command or path
- **THEN** it is never executed; only `run` from the config file is
