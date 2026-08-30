# Release process

Releases are crate-first and use release-plz with crates.io Trusted Publishing. No long-lived crates.io token belongs in repository or environment secrets.

1. Publish the pinned `lenso-capability-support-case` version first. Until it exists in crates.io, Cargo cannot assemble this public package from registry dependencies; CI therefore checks the exact package source set with `cargo package --list`.
2. Merge only after CI, repository-boundary, and public-package checks pass.
3. Let the `release-plz-pr` job prepare the version PR.
4. Merge the version PR.
5. In crates.io, configure a Trusted Publisher for repository `LioRael/lenso-support-web-plugin`, workflow `release-plz.yml`, environment `release`.
6. Protect the GitHub `release` environment and require the intended reviewers.
7. Run the Release-plz workflow on `main` with `live=true` and `confirm=publish`.
8. Verify the crate version and immutable Git tag `lenso-support-web-plugin@<version>`.

The workflow grants `id-token: write` only to the confirmed live release job. Its dry-run and release-PR jobs cannot publish.
