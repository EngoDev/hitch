# Hitch GitHub Workflows Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add GitHub Actions workflows for CI and tagged Linux releases for `x86_64` and `aarch64`.

**Architecture:** Keep CI and release publishing in separate workflow files. CI remains a simple validation job, while the release workflow uses a build matrix, creates a GitHub Release on `v*` tags, and uploads one binary per target.

**Tech Stack:** GitHub Actions, stable Rust toolchain, Linux build matrix, GitHub release/upload actions.

---

### Task 1: Add CI Workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Step 1: Write the failing verification**

Create the workflow file expectation and verify the repo currently has no CI workflow.

Run: `test -f .github/workflows/ci.yml`
Expected: FAIL

**Step 2: Write minimal implementation**

Add a GitHub Actions workflow that:

- runs on `push`
- runs on `pull_request`
- checks out the repo
- installs stable Rust
- runs `cargo test`

**Step 3: Verify the workflow file exists**

Run: `test -f .github/workflows/ci.yml`
Expected: PASS

### Task 2: Add Release Workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1: Write the failing verification**

Run: `test -f .github/workflows/release.yml`
Expected: FAIL

**Step 2: Write minimal implementation**

Add a release workflow that:

- triggers on tags matching `v*`
- uses a matrix for:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
- installs the target
- builds `cargo build --release --target <triple>`
- renames the resulting binary to include the tag and target
- creates or updates a GitHub Release
- uploads both binaries as assets

**Step 3: Verify the workflow file exists**

Run: `test -f .github/workflows/release.yml`
Expected: PASS

### Task 3: Verify Workflow Content And Repo State

**Files:**
- Modify: `README.md` (optional, only if release usage should be documented)

**Step 1: Write the failing verification**

Verify the workflows contain the required triggers and target triples.

Suggested checks:

```bash
rg "pull_request|push" .github/workflows/ci.yml
rg "v\\*" .github/workflows/release.yml
rg "x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu" .github/workflows/release.yml
```

**Step 2: Update as needed**

Adjust the workflow YAML until those checks pass.

**Step 3: Final verification**

Run:

```bash
test -f .github/workflows/ci.yml
test -f .github/workflows/release.yml
rg "pull_request|push" .github/workflows/ci.yml
rg "v\\*" .github/workflows/release.yml
rg "x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu" .github/workflows/release.yml
```

Expected:

- both workflow files exist,
- CI includes `push` and `pull_request`,
- release includes `v*`,
- release references both Linux targets.
