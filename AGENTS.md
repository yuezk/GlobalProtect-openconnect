# GlobalProtect-openconnect

A GlobalProtect VPN client for Linux (CLI + GUI), built on OpenConnect with SSO support.
Rust workspace, plus a Tauri/TypeScript GUI helper. Mixes native C (vendored OpenConnect/libxml2
via submodules) with Rust via `build.rs` bindings.

## Commands

| Command | What it does |
|---------|--------------|
| `make build` | Build everything (downloads/builds GUI helper, then `cargo build --release --workspace`) |
| `make build BUILD_GUI_HELPER=0` | Build CLI components only (`gpclient`, `gpservice`, `gpauth`), no GUI |
| `cargo build --release --workspace` | Build the Rust workspace directly (what CI runs) |
| `sudo make install` | Install built binaries |
| `make tarball` | Produce a release source tarball |
| `./target/release/gpclient --help` | Smoke-test a CLI build |

There is no `cargo test`/`cargo clippy`/`cargo fmt` step in CI (`.github/workflows/build.yaml`
only runs `cargo build --release --workspace`), and only `crates/gpapi` has a `tests/` directory.
Run `cargo fmt` locally before committing anyway — `rustfmt.toml` sets non-default style
(2-space indent, 120 col width) that `cargo fmt` enforces.

Building needs submodules: `git submodule update --init --recursive` (OpenConnect and libxml2
are vendored as git submodules under `crates/openconnect/deps/`). The DevContainer
(`.devcontainer/`) has all native build deps preinstalled; local builds need the OpenConnect
build toolchain (autoconf, automake, libxml2, gnutls, etc. — see README "Building from Source").

## Layout

- `crates/` — Rust library crates: `openconnect` (FFI bindings + vendored C submodules),
  `gpapi` (portal/gateway API client, has the only tests), `auth`, `common`.
- `apps/gpclient`, `apps/gpservice`, `apps/gpauth` — Rust binaries (CLI, privileged background
  service, auth helper).
- `apps/gpgui-helper` — Tauri app (`src-tauri/`, Rust) wrapping a separate TypeScript frontend
  (`src/`); the actual GUI (`gpgui`) is a separate downloaded artifact (`make download-gui`), not
  built from this repo's TS sources directly.
- `packaging/` — distro packaging (deb/rpm/pkgbuild/apk/BSD).
- `scripts/` — release and CI helper scripts (gh-release, flake hash updates, etc.).
- `flake.nix` — NixOS module + package definitions; also used by `nixos-rebuild` consumers.

## Conventions

- Rust edition 2024, `rust-version = "1.89"` (workspace-pinned in `Cargo.toml`).
- `rustfmt.toml`: 2-space indent (not 4), 120-column width, Unix newlines, imports/modules reordered.
- C/C++ (`.clang-format`): LLVM base, 8-wide tabs, tabs not spaces, 80-column width.
- Workspace dependencies are centralized in root `Cargo.toml` under `[workspace.dependencies]` —
  add new deps there, then reference with `workspace = true` in the crate's own `Cargo.toml`.
- Release profile favors small binaries: `opt-level = 'z'`, LTO, `panic = 'abort'`, symbols stripped.

## Gotchas

- Submodules are required to build `crates/openconnect` — a plain `git clone` without
  `--recursive` (or a forgotten `submodule update --init --recursive`) fails the build with
  missing OpenConnect/libxml2 sources.
- `gpgui-helper`'s TS frontend (`apps/gpgui-helper/src`) is scaffolding; the real GUI bundle is
  fetched separately (`make download-gui`), so editing `src/` won't change what ships unless
  that's the GUI you're actually building against.
- CI only builds, never tests or lints — a passing `cargo build` is not evidence of passing tests;
  check `crates/gpapi/tests` manually if you touch that crate.
- `gpservice` runs privileged; `gpclient hip` is exempted from the client's singleton-lock check
  (see recent commit history) — don't assume all subcommands share one lock.

## Git

- Branch: `main` is default; releases also use `hotfix/*`, `feature/*`, `release/*`.
- Commit style: Conventional Commits (`feat:`, `fix:`, `fix(scope):`, `chore:`, `ci:`, `doc:`),
  usually squashed from a PR with `(#NNN)` suffix referencing the PR number.
