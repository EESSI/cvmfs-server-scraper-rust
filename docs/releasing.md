# Releasing to crates.io

Releases use Git tags named `v<VERSION>`, matching `[package].version` in
`Cargo.toml`. Pushing a `v*` tag runs the
[publishing workflow](../.github/workflows/publish.yml), which checks the version,
runs the default test suite, verifies the packaged crate, and publishes it using
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing).

The [Rust CI workflow](../.github/workflows/rust.yml) runs tests and a package dry
run on pull requests and `main` without publishing or requesting an OIDC token.
Only tag pushes in `EESSI/cvmfs-server-scraper-rust`
can start the publishing job. The job uses the `crates-io` GitHub environment and
the [official authentication action](https://github.com/rust-lang/crates-io-auth-action)
to obtain a short-lived token, which the action revokes when the job finishes.
No long-lived crates.io token is required in GitHub secrets.

## One-time maintainer setup

Before pushing the first tag that includes this workflow:

1. Have an EESSI administrator add
   `rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18`
   (v1.0.5) to the allowed actions in **Settings → Actions → General**. EESSI's
   current policy blocks this action; the publishing workflow cannot start until
   it is allowed. Keep this entry aligned when updating the action's pinned commit.
2. In the repository's **Settings → Environments**, create an environment named
   `crates-io`. Configure **Selected branches and tags** to allow tags matching
   `v*`. Add required reviewers if release approval is desired.
3. As an owner of `cvmfs_server_scraper`, open the crate's
   [settings on crates.io](https://crates.io/crates/cvmfs_server_scraper/settings)
   and add a GitHub publisher under **Trusted Publishing** with these values:

   | Field | Value |
   | --- | --- |
   | Repository owner | `EESSI` |
   | Repository name | `cvmfs-server-scraper-rust` |
   | Workflow filename | `publish.yml` |
   | Environment | `crates-io` |

Use the workflow filename alone, without `.github/workflows/`. The environment
name must match both the workflow and the crates.io publisher configuration.
These settings live outside Git and are not installed by merging this workflow.
The crate already exists on crates.io, so no initial manual publish is needed.

## Preparing a release

1. Update `main` and choose a new, unpublished version. Update `Cargo.toml` and
   the root package version in `Cargo.lock` together.
2. Move the relevant `Changelog.md` entries from `Unreleased` into a dated release
   section. Update the README with the release version, date, highlights, release
   links, and any changed installation or migration requirements.
3. Run the documented development checks and `cargo publish --locked --dry-run
   --registry crates-io`. Merge the release preparation PR and wait for CI on
   `main` to pass, including the CVMFS testbed checks.
4. Create and push a signed tag on the release commit. For example, for a prepared
   `0.0.8` release:

   ```sh
   git switch main
   git pull --ff-only origin main
   git tag -s v0.0.8 -m "Release 0.0.8"
   git push origin v0.0.8
   ```

5. Watch **Actions → Publish to crates.io**, approve the environment deployment
   if required, and confirm the new version appears on crates.io.

GitHub Release objects are optional: the tag push is the publishing trigger.
Creating or editing a GitHub Release for that tag does not publish the crate a
second time. A tag/version mismatch fails before publishing authentication.

## Existing tags and retries

Versions `v0.0.1` through `v0.0.7` already exist as Git tags and are published on
crates.io. Adding this workflow does not replay those tags or republish them.
Create the next release tag from a commit that includes the workflow; do not move
or recreate an existing release tag.

If a run fails before uploading the crate, correct any external setup problem
and rerun the failed jobs for that tag. If the upload may have succeeded, check
crates.io first: published versions cannot be overwritten, and retrying a version
that already exists will fail. Source or version changes require a new release
commit and a new tag.
