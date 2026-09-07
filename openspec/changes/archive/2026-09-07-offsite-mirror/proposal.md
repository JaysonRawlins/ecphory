## Why

The git mirror is a faithful cold tier, but it lives on the same disk as the
store: the two existing durability layers (version history, mirror) share
one failure mode — the machine. For a distributable memory product, offsite
must be a first-class, near-zero-config story, and it must not violate the
design axioms (single binary, no SDK dependencies, no credential custody).

## What Changes

- **Auto-push after committed exports**: if the mirror has a git remote
  configured, push to it (`push <remote> HEAD`). Adding the remote is the
  entire setup and the entire opt-in. Push failures warn and never fail the
  export. `ECPHORY_EXPORT_PUSH=false` disables.
- **A store-wide `export` trigger event**: fires after every committed
  export with `ECPHORY_TRIGGER_EXPORT_DIR` in the command's environment.
  Needs no episode matcher. This is the universal adapter: S3, restic,
  rclone, anything — operator's command, operator's credentials.
- **docs/backups.md**: the three-tier story (version history → local git
  mirror → offsite), recipes for git-remote and S3/restic-via-trigger, and
  the restore drill.

## Non-goals

- Native object-storage clients (SDK bloat, credential custody, multi-cloud
  matrix — the trigger gets every backend without any of it).
- Encryption of the mirror (an operator choosing git-crypt/restic gets it in
  their layer).

## Capabilities

### New Capabilities

- `offsite-mirror`: committed exports propagate to an operator-configured
  offsite destination automatically, via git remote or export trigger.

## Impact

- `export::git_push` + `push_enabled`; scheduled and CLI export paths gain
  push + `export` trigger firing. Triggers gain the store-event class.
- No new dependencies; git remains the only external tool, already required
  by the mirror.
