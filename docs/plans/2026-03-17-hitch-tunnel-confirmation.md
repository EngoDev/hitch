# Hitch Tunnel Confirmation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an `inquire`-based confirmation and edit flow before Hitch opens an auto-detected tunnel in wrapped-command mode.

**Architecture:** Keep URL detection pure and insert a tunnel-configuration confirmation step immediately before tunnel launch. Use a small abstraction so runtime tests can exercise acceptance, editing, validation, and `--port` bypass behavior without requiring an interactive terminal.

**Tech Stack:** Rust 2024, `inquire`, existing runtime/tunnel abstractions, existing TTY handling.

---

### Task 1: Add Tunnel Configuration Model And Editor Abstraction

**Files:**
- Create: `src/confirm.rs`
- Modify: `src/runtime.rs`
- Modify: `Cargo.toml`

**Step 1: Write the failing tests**

Add tests for:

- accepting detected values unchanged,
- editing values in the order `port`, `user`, `origin`,
- clearing user with an empty response,
- invalid port input being rejected.

Suggested test shape:

```rust
#[test]
fn editor_applies_port_user_origin_edits_in_order() {
    let detected = TunnelConfig {
        port: 3001,
        user: Some("engodev".into()),
        origin: "100.70.126.5".into(),
    };

    let editor = ScriptedEditor::new_confirm_no(["4000", "", "203.0.113.10"]);
    let result = confirm_tunnel_config(&editor, detected).unwrap();

    assert_eq!(result.port, 4000);
    assert_eq!(result.user, None);
    assert_eq!(result.origin, "203.0.113.10");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- --nocapture`

Expected: FAIL because the config/editor abstraction does not exist yet.

**Step 3: Write minimal implementation**

Implement:

- `TunnelConfig` model
- confirmation/editor trait
- `inquire` implementation for real prompts
- validation helpers for `port`, `user`, and `origin`

**Step 4: Run test to verify it passes**

Run: `cargo test --lib -- --nocapture`

Expected: PASS

**Step 5: Re-run the full test suite**

Run: `cargo test`

Expected: PASS

### Task 2: Integrate Confirmation Into Auto-Detected Tunnel Launch

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/status.rs`

**Step 1: Write the failing tests**

Add runtime tests for:

- auto-detected tunnel launch with confirmation accepted,
- auto-detected tunnel launch with edits applied,
- `--port` mode bypassing confirmation.

Suggested test shape:

```rust
#[test]
fn detected_tunnel_uses_edited_values_before_launch() {
    let launcher = FakeTunnelLauncher::new();
    let editor = ScriptedEditor::new_confirm_no(["4000", "", "203.0.113.10"]);
    let mut runtime = Runtime::new_for_test(launcher.clone(), editor, Some("100.70.126.5".into()), Some("engodev".into()));

    runtime.on_output("Waiting for callback on http://localhost:3001/auth/callback");

    assert_eq!(launcher.started_ports(), vec![4000]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test runtime:: -- --nocapture`

Expected: FAIL because runtime launches tunnels directly without confirmation.

**Step 3: Write minimal implementation**

Implement:

- printing detected tunnel details before launch,
- confirmation/edit hook inside the wrapped-command tunnel path,
- `--port` mode bypass behavior

**Step 4: Run test to verify it passes**

Run: `cargo test runtime:: -- --nocapture`

Expected: PASS

**Step 5: Re-run the full test suite**

Run: `cargo test`

Expected: PASS

### Task 3: Update Help And README, Then Verify

**Files:**
- Modify: `README.md`
- Modify: `src/lib.rs`

**Step 1: Write the failing tests**

Add tests for README/help mentioning that auto-detected tunnel details are confirmed before launch.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- --nocapture`

Expected: FAIL until docs are updated.

**Step 3: Write minimal implementation**

Update README and any help/status wording needed to describe the confirmation flow.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib -- --nocapture`

Expected: PASS

**Step 5: Final verification**

Run:

```bash
cargo test
```

Expected:

- all tests pass,
- auto-detected tunnel launch now has a confirmation/edit hook,
- `--port` mode remains unchanged.
