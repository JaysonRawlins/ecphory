## Why

ecphory's factory pinned swamp's identity in the repo: a committed `.envrc`
loaded the personal swamp-club key from Doppler, and `scripts/swamp` refused to
run without it. Any call that missed that route (another directory, a shell
without direnv, a driver-skill line saying bare `swamp`) fell back to the
employer login in swamp's default config dir.

That fallback no longer exists. The employer login moved to its own config dir,
so a session that carries no identity is unauthenticated rather than wrong. The
identity now belongs to the session: a personal session starts under
`doppler run -p swamp -c personal`, which supplies both `SWAMP_CONFIG_DIR` and
`SWAMP_API_KEY`.

Separately, three workflows interpolated `${{ inputs.changeSlug }}` and one
interpolated `${{ inputs.commit }}` straight into a shell command. Run live in
this repo, `changeSlug="x; touch <marker>"` created the marker and the run
reported SUCCEEDED (run 3bc10ea4): an injected slug ran code and turned a gate
green. The slug is chosen by an agent reading an issue someone else wrote.

## What Changes

- **BREAKING (for the maintainer's workflow)**: remove `.envrc`, `scripts/swamp`
  and `tests/factory_swamp_wrapper.rs`. swamp is called as plain `swamp` inside
  a session started under `doppler run -p swamp -c personal`.
- The intake stage runs `swamp auth whoami --json` first and stops unless it
  names the maintainer's personal account.
- Every workflow value reaches its shell command only through the step's `env`,
  double-quoted. plan-verify now runs `scripts/factory/validate-change.sh`, and
  all three OpenSpec gate scripts refuse a slug that is not
  `[a-z0-9][a-z0-9-]*` with exit 2.

## Impact

- No change to the binary, its store format or its MCP, REST or CLI surfaces.
- The `tools: []` pin on `.swamp.yaml` moves to `tests/factory_definition.rs`.
- `~/.config/direnv/direnv.toml`'s ecphory whitelist becomes dead; it is the
  maintainer's machine config, removed outside this change.
