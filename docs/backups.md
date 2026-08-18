# Backups

Three tiers, from closest to farthest:

1. **Version history (in-store)** — every `update`/`demote` archives the
   prior state first; `restore_version` rolls back exactly. Protects
   against bad edits.
2. **The git mirror (same machine)** — `ECPHORY_EXPORT_DIR` exports every
   episode as deterministic markdown and commits when anything changed
   (at daemon start, then every `ECPHORY_EXPORT_INTERVAL`, default 24h).
   `ecphory import --dir` rebuilds a store from it, idempotently. Purges
   are git-gated: the mirror commits before anything is destroyed.
   Protects against store corruption and fat-fingered purges.
3. **Offsite** — the mirror leaves the machine. Protects against the disk
   dying, which is the failure the first two tiers share. This page is
   about tier 3.

## The one-liner: a git remote

The mirror is already a git repo, so offsite is one command:

```bash
cd "$ECPHORY_EXPORT_DIR"
git remote add origin git@github.com:you/ecphory-mirror.git   # private repo
```

That's the whole setup. After every committed export, ecphory pushes to the
first configured remote (`push -q <remote> HEAD`, so any branch name works).
Adding the remote *is* the opt-in; no remote means no push and no warning.
A push failure is logged and never fails the export — the local mirror is
already durable. Set `ECPHORY_EXPORT_PUSH=false` to disable pushing without
removing the remote.

Any git host works: GitHub/GitLab private repo, Gitea, or a bare repo on a
NAS (`git init --bare` + an SSH path as the remote).

## Everything else: the `export` trigger event

For destinations that aren't git remotes (S3, restic, rclone, Backblaze —
anything), subscribe a [trigger](triggers.md) to the `export` event. It
fires after every *committed* export, receives `ECPHORY_TRIGGER_EXPORT_DIR`
in its environment, and needs no episode matcher (it's store-wide):

```json
{
  "triggers": [
    {
      "name": "s3-offsite",
      "events": ["export"],
      "run": ["/usr/local/bin/aws", "s3", "sync", "/home/me/.local/share/ecphory/mirror", "s3://my-bucket/ecphory-mirror", "--delete"],
      "timeout_seconds": 300
    }
  ]
}
```

Swap the command for `restic backup`, `rclone sync`, or anything else — the
credentials stay in your tooling and ecphory never learns what a bucket is.
This is deliberate: a native S3 client would mean an SDK dependency tree, a
credential story, and then the GCS/Azure/R2 matrix, all against the
single-binary design axiom. The hook supports every backend by supporting
none of them.

## Restore

```bash
ecphory import --dir /path/to/mirror   # fresh machine: clone the mirror first
```

Import is idempotent (existing ids are skipped), and the mirror format
round-trips everything including `search_phrases` enrichment. Version
history and the flight recorder are **not** in the mirror — they are
tier-1/operational state, not memory content.

Test the restore path before you need it: clone your offsite mirror to a
temp dir, import into a scratch store (`--db /tmp/scratch.redb`), and
`ecphory status` should report your episode count.
