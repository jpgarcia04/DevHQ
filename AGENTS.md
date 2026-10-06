# DevHQ

For Codex and any other agent that reads `AGENTS.md`. The rules for working in this repository
live in [`CLAUDE.md`](CLAUDE.md): it is written for Claude Code, but every rule in it applies to
you too, so read it whole before changing anything and follow it as if it were addressed to you.
Nothing is copied here, so the two files can never disagree; a rule changes in `CLAUDE.md`, and
this file only points.

- [`README.md`](README.md) (in Spanish) is the specification. Read it before changing
  behaviour and update it in the same commit as the change.
- [`docs/vision.md`](docs/vision.md) (in Spanish) holds the long-term direction and the
  questions to ask of any new feature or architectural change. Read it before proposing either.
  It guides and authorizes nothing.

What in `CLAUDE.md` belongs to Claude Code alone:

- «Cloud sessions»: `.claude/hooks/session-start.sh` is the SessionStart hook of Claude Code on
  the web. In any other Linux container that lacks WebKitGTK, WebKitWebDriver, Xvfb, tauri-driver
  or npm 11, run it by hand with the command given there.
- «Style» asks for the session trailers at the end of each commit message: use the ones your own
  tool adds, if any.
- «Local Windows sessions» applies to the Codex desktop app as much as to Claude's: both are
  packaged (MSIX), and Windows redirects AppData for every process they start.
