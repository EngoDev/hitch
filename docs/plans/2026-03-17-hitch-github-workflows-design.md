# Hitch GitHub Workflows Design

**Date:** 2026-03-17

## Goal

Add GitHub Actions workflows for:

- continuous integration on pushes and pull requests,
- tagged releases that build Linux binaries for `x86_64` and `aarch64` and attach them to a GitHub Release.

## CI Workflow

The CI workflow should:

- run on `push` and `pull_request`,
- install stable Rust,
- run `cargo test`.

This workflow is validation-only and does not publish artifacts.

## Release Workflow

The release workflow should:

- run on pushed tags matching `v*`,
- build a release binary for:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
- create a GitHub Release for the tag,
- upload the binaries as release assets.

## Asset Naming

Asset names should include:

- project name,
- tag name,
- target triple.

Example:

- `hitch-v0.1.0-x86_64-unknown-linux-gnu`
- `hitch-v0.1.0-aarch64-unknown-linux-gnu`

## Implementation Notes

The release workflow will likely need:

- a build matrix,
- target installation via `rustup target add`,
- a cross-linker/toolchain path for `aarch64-unknown-linux-gnu`,
- `permissions: contents: write` so the workflow can create releases and upload assets.

## Testing Strategy

Validation should focus on:

- workflow syntax correctness,
- CI invoking `cargo test`,
- release workflow referencing the correct target triples and asset paths.
