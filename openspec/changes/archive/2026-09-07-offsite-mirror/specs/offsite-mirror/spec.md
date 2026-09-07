# offsite-mirror Specification

## ADDED Requirements

### Requirement: A committed export propagates offsite automatically
After a committed mirror export, the system SHALL push the mirror to its first configured git remote (unless `ECPHORY_EXPORT_PUSH` disables it) and SHALL fire the `export` trigger event, so an operator-configured destination receives the new state without any scheduled job outside ecphory.

#### Scenario: Mirror with a remote
- **WHEN** a scheduled or CLI export commits new mirror state and the mirror has a git remote
- **THEN** the mirror is pushed to that remote and the export log records pushed=true

#### Scenario: No remote configured
- **WHEN** an export commits and the mirror has no git remote
- **THEN** no push is attempted, no warning is emitted, and the export succeeds

### Requirement: Offsite failures never compromise the export
A push failure or a failing export-trigger command SHALL be logged as a warning and SHALL NOT fail, delay indefinitely, or roll back the export or the commit.

#### Scenario: Remote unreachable
- **WHEN** the push fails (network down, auth expired)
- **THEN** the local mirror commit stands, the export reports success with pushed=false, and the failure is visible in the log

### Requirement: Store-wide trigger events need no episode matcher
A trigger subscribed only to store-wide events (`export`) SHALL be valid without an episode matcher, SHALL receive `ECPHORY_TRIGGER_EXPORT_DIR`, and SHALL never fire on episode events.

#### Scenario: S3 sync trigger
- **WHEN** a trigger with events ["export"] and no matcher is configured and an export commits
- **THEN** the command runs with the mirror path in its environment, and episode writes never invoke it
