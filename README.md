# martha-board

MarthaBoard: the board for the Martha methodology. It is an infinite-canvas board app, written in
Rust.

## Build and run

    cargo build --release -p figcraft -p figcraft-cli

This builds two binaries: `figcraft`, the desktop app, and `figcraft-cli`, the command line. Run
the app with `cargo run --release -p figcraft`. The crates and apps still carry FigCraft's names;
they change in the rename package.

## The plan

The plan is in the issues: [#1](https://github.com/macula-apps/martha-board/issues/1) (what was
taken from FigCraft, what stays and why) and [#4](https://github.com/macula-apps/martha-board/issues/4).

## History and licences

MarthaBoard starts from FigCraft's history at commit d4b4387. FigCraft's own README is in this
file's history (`git log --follow README.md`).

Licences: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE), [NOTICE](NOTICE) and
[ATTRIBUTION.md](ATTRIBUTION.md).
