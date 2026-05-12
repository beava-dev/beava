# Releasing Beava

Step-by-step for cutting a release and publishing the wheel to PyPI.

## TL;DR

```bash
# 1. Verify main is green + CHANGELOG entry is up to date
# 2. Tag and push
git tag v0.0.X
git push beava-dev v0.0.X

# 3. CI builds wheels (linux x86_64/aarch64, macOS aarch64/x86_64) and an sdist,
#    attaches them to the GitHub Release, and (if Trusted Publishing is enabled)
#    publishes to PyPI.
```

Three workflows fire on a `v*` tag push:

| Workflow | Job | Output |
|---|---|---|
| `release.yml` | `build-and-release` | Linux x86_64 standalone binary + GitHub Release |
| `release-wheels.yml` | `build`, `sdist` | Per-platform wheels + sdist as workflow artifacts |
| `release-wheels.yml` | `publish` | Publishes wheels + sdist to PyPI (gated, see below) |
| `release-wheels.yml` | `attach-to-release` | Attaches wheels + sdist to the GitHub Release alongside the binary |

The `publish` job is **gated** by the repo variable `PYPI_TRUSTED_PUBLISHER_READY`. Until it's `true`, the job silently skips. `curl ... | sh` install keeps working from the GitHub Release assets regardless.

## First-time PyPI setup (one-time, before the first publish)

You need a PyPI account, a Trusted Publisher configured on PyPI, and the gate flipped on the repo. Roughly 10 minutes.

### 1. PyPI account

- Sign up at https://pypi.org/account/register/ if you don't already have one.
- Enable 2FA (mandatory since 2024).

### 2. Add `beava` as a pending Trusted Publisher

Go to https://pypi.org/manage/account/publishing/ and fill in the **pending publisher** form:

| Field | Value |
|---|---|
| PyPI Project Name | `beava` |
| Owner | `beava-dev` |
| Repository name | `beava` |
| Workflow filename | `release-wheels.yml` |
| Environment name | `pypi` |

This pre-authorizes a workflow run from `beava-dev/beava` on the `pypi` environment to claim the name and publish. The first successful publish creates the project under your account. No API tokens are stored anywhere.

### 3. (Optional but recommended) Same setup on TestPyPI

Repeat step 2 at https://test.pypi.org/manage/account/publishing/ so you can dry-run releases via `workflow_dispatch` against TestPyPI before real publishes. Requires a small workflow tweak — track this in a follow-up if you want it.

### 4. Flip the gate

Once steps 1–2 are done:

```bash
gh variable set PYPI_TRUSTED_PUBLISHER_READY --repo beava-dev/beava --body "true"
```

The next `v*` tag push publishes to PyPI automatically.

## Per-release checklist

Before tagging:

- [ ] `CHANGELOG.md` has an entry for the new version
- [ ] `python/pyproject.toml` `version` field bumped to match the tag (drop the leading `v`)
- [ ] `cargo test --workspace` green locally and on `main`
- [ ] `mypy --strict beava` green
- [ ] The wheel builds cleanly via `release-wheels.yml` on a recent main push (look for the most recent workflow_dispatch run)

Tag format: `vMAJOR.MINOR.PATCH`. Pre-releases use `vX.Y.Z-rc1` / `-beta1` / `-alpha1` suffixes — the release workflow auto-marks those as GitHub pre-releases.

## After a release

- [ ] Verify the release page at https://github.com/beava-dev/beava/releases/tag/vX.Y.Z has the binary + wheels + sdist
- [ ] Verify `pip install beava==X.Y.Z` works (PyPI propagation is sub-minute typically)
- [ ] Verify `curl -fsSL .../install.sh | sh` resolves the new version
- [ ] Update the Homebrew formula (`homebrew-bump.yml` runs automatically on tag push)

## Troubleshooting

**`publish` job is skipped on tag push.** `PYPI_TRUSTED_PUBLISHER_READY` repo variable is not `true`. Check with `gh variable list --repo beava-dev/beava`. Flip per step 4 above if PyPI side is configured.

**`publish` job fails with `403 Forbidden`.** The pending publisher on PyPI doesn't match this repo + workflow + environment. Verify the four fields under step 2 match exactly (case-sensitive on names).

**`publish` job fails with `400 File already exists`.** Tag was re-pushed without bumping `version` in `pyproject.toml`. PyPI rejects re-uploads of the same `(name, version)` tuple by policy. Bump the version, force-delete the tag, re-tag, push.

**Wheel build matrix fails on linux/aarch64.** Cross-compile via qemu can flake. Re-run the failed job; if it persists, check `PyO3/maturin-action` issues for known toolchain regressions.

## Why Trusted Publishing instead of an API token

- No long-lived secret in the repo
- OIDC short-lived token per workflow run, scoped to the `pypi` environment
- Rotating credentials is irrelevant — there's no credential to rotate
- Industry standard for OSS releases since 2023

If for some reason you must fall back to an API token, set `PYPI_API_TOKEN` as a repo secret and replace `pypa/gh-action-pypi-publish@release/v1`'s usage to read `password: ${{ secrets.PYPI_API_TOKEN }}`. Strongly not recommended.
