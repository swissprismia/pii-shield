# CLAUDE.md

## Release Versioning

- Keep release metadata in sync before creating a release tag.
- Update `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` to the intended release version first.
- Tauri bundle filenames are derived from app metadata, not from the Git tag alone.
- If the metadata still says `0.1.0`, a `v0.3.0` tag will still produce artifacts named like `PII Shield_0.1.0_*`.
- Prefer creating a new tag such as `v0.3.1` instead of moving an already-published release tag.
