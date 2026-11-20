<!-- craft-house-rules:start core source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=387e0ccb4b49e567f60951d7d6fff7305054c89f43ad812b9993dc7c38528013 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# MarthaBoard — instructions for agents

MarthaBoard: the board for the Martha methodology. It is an infinite-canvas board app, written in
Rust with egui. It is built from FigCraft's crates and apps, which keep FigCraft's names until the
rename package. The plan is in the issues: macula-apps/martha-board#1 and #4.

Share reusable code through craft-libs. Do not depend on sibling application crates.

## Non-negotiables

### Clean room
- Behaviour comes from public documentation and black-box observation of a running app, with
  synthetic documents only. Never read, disassemble or copy anything inside another product's
  application bundle, web bundle or network traffic (names and public listings only).
- Never copy GPL or AGPL code.
- Screenshots of another product stay local. They are never committed, published or used as
  assets.

### Assets
- **Never add, copy, trace, redraw from, embed or ship any icon, image, illustration, cursor, logo,
  template, UI kit, preset library, font file or screenshot from Figma, Adobe, Avid, Autodesk or any
  other product, or from any source whose licence does not allow it.** This includes lookalike
  redraws of their icons.
- Every image, icon, font or other asset is one of: original work for this project (MIT OR
  Apache-2.0), OSI open source, public domain or CC0, or Creative Commons with redistribution
  allowed.
- **Every asset file has a row in [`ATTRIBUTION.md`](ATTRIBUTION.md)** (path, author, source,
  licence, notes). `cargo xtask assets` fails on any unlisted file under `assets/`.
- UI icons are drawn in code (`crates/ui-egui/src/icons.rs`) from simple primitives. New icons are
  new code, designed from scratch.
- The app icon is `assets/app-icon/` (see its README). `cargo xtask icon` renders the PNGs and
  `.ico` from `assets/app-icon/figcraft.svg`.
- **Fonts live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), never in this
  repo.** The build reads it through the optional `CRAFT_FONTS_DIR` input (`.cargo/config.toml`
  points it at `../craft-fonts`). Everything builds, tests and runs without it.
- Demo and test content is synthetic or public domain, never personal photos or data.
- Code and original assets are MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`).
- If the provenance of an asset is unclear, do not add it.

### Never crash
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` in shipped code.
  The workspace clippy lints deny them; tests may use them. Return `Result` or `Option`, use
  `.get()` for indexes from data, slice strings only at char boundaries, check floats are finite
  and cap sizes.
- New workspace crates set `[lints] workspace = true` in `Cargo.toml`.
- `unsafe_code = "forbid"` holds workspace-wide.
- `figcraft_engine::guard` turns a panicking command into `EngineError::Internal` and rolls the
  document back. It is a safety net, not a substitute for `Result`.
- Every crash fix comes with a regression test. The engine sweeps junk parameters over every
  command (`crates/engine/src/tests.rs`); extend it when you add parsers or commands.

### Architecture
- **Every action is a command.** User-visible behaviour is a command in `crates/engine/src/cmd/*`
  (`spec!(id, label, params doc, undoable, run)`) with tests. The menus, shortcuts, Actions
  palette, CLI, control channel and MCP server all dispatch the same command ids. The menu tree is
  `crates/engine/src/catalog.rs`.
- **Programmatic calls never open dialogs.** Commands only request UI (`Session::request_ui`); the
  control channel drops dialog requests.
- **Layering** is checked by `cargo xtask layers`: geom and color (L0), doc (L1), layout, text,
  pathops and collab (L2), render and format (L3), svg and pdf (L4), engine (L5), ui-egui
  and mcp (L6), then the apps. Nothing below L6 depends on egui, eframe, rfd or winit.
- **The UI is thin.** Panels read engine state and act through `app.run(id, params)` and
  `app.set_props(ids, props)`. Colours come from `theme::Tokens`.

### Tests and checks
- Every change comes with its tests, in the same commit.
- CI is the check: `.github/workflows/ci.yml` runs fmt, clippy with the structural caps, the build
  and the whole test suite on every pull request. A change is done when that run is green on the
  pull request's tip.
- Locally: `cargo xtask ci` runs fmt, clippy `-D warnings`, the tests, `layers`, `assets` and
  `docs`. `cargo xtask` with no task lists the others.

## Building and running
- `cargo build --release -p figcraft -p figcraft-cli` builds the app (`figcraft`) and the command
  line (`figcraft-cli`).
- `cargo run -p figcraft -- --control <port> [file]` starts the app with its JSON-lines control
  channel. Its methods are in `crates/ui-egui/src/control.rs` and `docs/control-protocol.md`.
- For UI work, look at the result: take a screenshot through the control channel and read it.
- Headless: `figcraft-cli` (`info`, `export`, `run`, `commands`, `mcp`; see `docs/cli.md` and
  `docs/mcp.md`).
- Parallel agents: a separate `CARGO_TARGET_DIR` per agent, and edit only the files you own.

## Contributor credits (About window)
- About ▸ Contributors is compiled from `contributors/contributors.json` (commit stats; never
  hand-edit) and `contributors/people.toml` (names people chose for themselves).
- When you prepare a PR for a contributor, check whether their GitHub username has a
  `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once whether they want
  more than their username credited. If yes, add only their own entry, committed as them. If no,
  change nothing.
- Never add, edit, guess or copy anyone else's entry or name.
