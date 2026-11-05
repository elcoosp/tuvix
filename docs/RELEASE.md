# Release process

Releases are automated by [`.github/workflows/release.yml`](../.github/workflows/release.yml)
on tag push (`v*`) and use git-cliff 2.14 (via `orhun/git-cliff-action@v4`)
for changelog generation.

## Steps

1. Ensure CI is green on `main`.
2. Bump versions (all four crates share the workspace version in
   `Cargo.toml`), commit:

   ```
   git commit -m "chore(release): prepare for v0.2.0"
   ```

3. Tag (annotated):

   ```
   git tag -a v0.2.0 -m "Release v0.2.0"
   git push origin main --tags
   ```

4. The workflow then:
   - generates the release notes for the latest tag with
     `git-cliff --latest --strip header`,
   - creates the GitHub release with those notes,
   - publishes `tuvix-core` → `tuvix-widgets` → `tuvix-backend-crossterm` →
     `tuvix` to crates.io in dependency order (requires the
     `CARGO_REGISTRY_TOKEN` secret; skipped cleanly when absent).

## Local changelog preview

With git-cliff installed:

```
git cliff -o CHANGELOG.md
```

The configuration lives in [`cliff.toml`](../cliff.toml) and maps
Conventional Commits to Keep a Changelog groups.

## Versioning

Semantic Versioning. Breaking changes come from `feat!:`/`fix!:` commits or
`BREAKING CHANGE:` footers and are highlighted in the changelog
(`protect_breaking_commits` in cliff.toml).
