# Writer

A Mac app that writes video scripts from footage. Import a YouTube link, a Drive folder or a file, and Writer transcribes it, has Claude build the script the way Repack writes them, and cuts the footage on your Mac. Every API key lives on the team's relay, so there's nothing to set up beyond signing in.

How every part works: [docs/how-it-works.md](docs/how-it-works.md).

## Install

1. Install [Claude Code](https://claude.com/claude-code) and run `claude` once to sign in. Writer uses that login.
2. Download `Writer-Apple-Silicon.dmg` (or `Writer-Intel.dmg`) from [Releases](https://github.com/spheceo/writer/releases) and drag Writer to Applications.
3. Open Writer and sign in with Google at accounts.spheceo.com. An admin has to add you first.

Writer updates itself from Releases.

## Develop

Needs [Bun](https://bun.sh), [Rust](https://rustup.rs), [just](https://github.com/casey/just) and ffmpeg (`brew install ffmpeg`).

```sh
bun install
just dev      # the app with hot reload
just check    # typecheck and tests, before committing
just bundle   # the app to hand out, ffmpeg inside
```

Releases (`just nightly`, `just publish`) are covered in [AGENTS.md](AGENTS.md).
