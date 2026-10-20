## Context

Placet made the same change first (its branch `chore/swamp-factory`); this
follows its shape in Rust tests.

## Decisions

**The session carries the identity, not the repo.** The wrapper and `.envrc`
had to be on every path swamp was reached by, and each path they missed was a
silent fallback. With the employer login out of the default config dir, the
fallback is "not authenticated", which fails loudly on its own. Wrapping the
session (`doppler run ... -- claude`) covers every call it makes.

**intake checks whoami before anything else.** A session started some other
way, or one that still finds another login, is stopped before the run reads
the issue. It is a prompt rule, pinned by `factory_definition`; there is no
swamp gate type that could check the caller's identity.

**Values go through `env`, never into the command text.** A workflow input's
`pattern` is not enforced by swamp (measured in placet), so it cannot be the
guard. With the value in the environment and the command using `"$NAME"`, the
shell never parses it. The gate scripts also refuse a malformed slug, so a
value that is not a slug fails as a usage error even where it would be inert.

**plan-verify gets a script.** It ran `openspec validate` inline, the only
OpenSpec gate without a script to hold the slug check. `validate-change.sh`
gives it one.

## Risks

- A session not started under `doppler run` cannot drive the factory at all
  until restarted, or each call prefixed. That is the intended failure.
