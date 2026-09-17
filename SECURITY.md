# Security

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: the **Report a vulnerability**
button on the [Security tab](https://github.com/JaysonRawlins/ecphory/security/advisories/new).
It opens a private thread with me, and nothing becomes public until there is
something worth publishing.

Please do not open a regular issue for a vulnerability. Issues are the
contribution channel for everything else here (see
[CONTRIBUTING.md](CONTRIBUTING.md)) and they are world-readable the instant you
press the button, which is the one case where that is the wrong door.

This is a single-maintainer project with no rotation behind it. Expect a first
response within a week. If a week passes in silence, open a public issue saying
only that you are waiting on a security report, with no details in it, and that
will reach me.

Only the latest release gets fixes. There are no maintained release branches.

## The threat model, so you can aim

ecphory is a single-user daemon holding one person's memories on their own
machine. The boundary is the loopback binding, not the token.

`ecphory serve` binds `127.0.0.1` and only `127.0.0.1`. That is hard-coded at
the listener rather than exposed as a flag or an environment variable, so no
configuration of ecphory serves your store to a network. `--port` and
`$ECPHORY_PORT` move the port; nothing moves the address.

Inside that boundary the data plane is open unless you opt in to
`ECPHORY_AUTH_TOKEN`. That is deliberate. A process running as you on your own
machine can open `~/.local/share/ecphory/ecphory.redb` directly, so a gate on
the port would not be protecting the store from it. The token matters when the
port stops being loopback-only, which is usually not ecphory's doing: an SSH
tunnel, a container port map, a VM forward. See the README's Security section
for how to set it and exactly what it covers.

At rest the store is an ordinary unencrypted file whose protection is
filesystem permissions. The flight recorder tapes real queries and episode ids,
so it carries whatever was private about them. The git mirror
(`ECPHORY_EXPORT_DIR`) writes episodes to a repository in plaintext, and they
travel wherever that repository is pushed.

### In scope

- Anything that lets code reach the store or the daemon that should not: a path
  that escapes the loopback bind, a way to make the daemon listen elsewhere.
- Bearer auth bypass on `/api/v1/*` when `ECPHORY_AUTH_TOKEN` is set.
- A panic, hang, or memory-safety fault reachable from a well-formed request,
  a crafted episode, or a corrupt store file.
- Secrets or episode content escaping somewhere you would not expect them:
  logs, error strings, the exported mirror, a rendered subject index.
- Anything in the release pipeline: a tampered artifact, a workflow that can be
  driven by an outside contributor.

### Out of scope

- **The data plane being open by default on loopback.** Documented above and in
  the README, and working as intended. A report that `curl` against
  `127.0.0.1:3491` returns episodes without a token is this, not a finding.
- **`/mcp` not being behind `ECPHORY_AUTH_TOKEN`.** Known and tracked in
  [#47](https://github.com/JaysonRawlins/ecphory/issues/47), and stated in the README rather than left for you to
  discover.
- **Another user on a shared machine reading the store file.** Filesystem
  permissions are the boundary there, and ecphory does not try to add a second
  one.
- Access control on wherever you push your mirror repository.
- Dependency advisories with no reachable path through ecphory. The Cargo tree
  is already scanned weekly by
  [security-scan](.github/workflows/security-scan.yml); a Grype line number is
  not by itself a report.
