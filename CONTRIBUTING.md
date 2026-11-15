# Contributing

Bug reports, documentation fixes, test cases and pull requests are welcome.
For a parser or mapping bug, include the crate versions, enabled features,
the smallest KDL document that shows it, and the Rust type you deserialize
into. Report security issues privately (see [SECURITY.md](SECURITY.md)).

Keep pull requests small and focused, with a commit subject that reads well
as a changelog line (release-plz builds CHANGELOG.md from them). Coding
conventions, the crate layout and the rules for error codes, snapshots and
mapping changes are in [AGENTS.md](AGENTS.md).

## Checks

The minimum Rust is 1.85 for kdl-tree, kdl-span, kdl-load and kdl-nodes and
1.95 for the rest (see the README). The Nix dev shell pins the toolchain CI uses and
provides cargo-nextest, cargo-deny and cargo-insta:

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo nextest run --workspace --all-features
nix develop -c cargo test --workspace --all-features --doc
RUSTDOCFLAGS="-D warnings" nix develop -c cargo doc --workspace --no-deps --all-features
nix develop -c cargo deny check
```

CI also runs clippy and the tests without `--all-features`, each crate with
`--no-default-features`, the pinned official KDL test suite, each crate built
and tested on its own MSRV, and `cargo publish --workspace --dry-run`. AGENTS.md lists
the commands.

## Releases

All seven crates share one version. `release-plz.toml` keeps them in one
version group, writes one root `CHANGELOG.md`, and has `kdl-serde` own the
`v<version>` tag and GitHub release. With `release_always = false`, only a
merged release PR publishes. Review the version bump (cargo-semver-checks
runs on the release PR) and the changelog, and merge only after CI passes.
release-plz publishes in dependency order: kdl-tree, kdl-span, kdl-load,
kdl-nodes, kdl-pretty, kdl-serde, kdl-pretty-cli.

The workflow runs only on the public `talaagent` repository.

Release PRs use the built-in `GITHUB_TOKEN` by default, with job-level
`contents: write` and `pull-requests: write` permissions. Enable **Allow
GitHub Actions to create and approve pull requests** in the organization and
repository Actions settings, then set the repository variable
`RELEASE_PR=true`. The workflow dispatches CI for those PRs itself (with
`actions: write`), because events from `GITHUB_TOKEN` don't trigger
workflows. An optional `RELEASE_PLZ_TOKEN` secret (GitHub App token or PAT)
replaces the built-in token.

### First publication

Trusted publishing can't create a crate, so the first release of each crate
needs a token:

1. Create a short-lived crates.io token with the `publish-new` scope and
   store it as the `CARGO_REGISTRY_TOKEN` Actions secret.
2. Add the release date to the `0.1.0` heading in CHANGELOG.md.
3. Run the release-plz workflow by hand with `bootstrap=true`. It publishes
   the committed versions. `release_packages` can limit a run to a
   comma-separated subset, for example to publish dependencies before
   their dependents.
4. On crates.io, add a trusted publisher to every crate: owner `talaagent`,
   repository `kdl-kit`, workflow `release-plz.yml`, no environment.
5. Delete the `CARGO_REGISTRY_TOKEN` secret and revoke the token. Later
   releases use OIDC. A persistent token would override it.

Afterwards, check each crate's page on crates.io and its docs.rs build. A
crate added to the workspace later goes through the same steps.

## License

Contributions are licensed under Apache-2.0, like the project. Keep the
club-kdl attribution in NOTICE and the header of `crates/kdl-tree/src/parse.rs`.
