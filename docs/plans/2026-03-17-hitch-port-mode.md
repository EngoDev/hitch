# Hitch Port Mode Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `--port <port>` mode that opens a reverse SSH tunnel directly, without wrapping a command, and keeps it alive until interrupted.

**Architecture:** Introduce an explicit config mode enum so CLI parsing selects either wrapped-command execution or port-only tunneling. Keep wrapped-command behavior intact and add a small port-mode runtime path that launches one tunnel, waits for interrupt, and shuts it down cleanly.

**Tech Stack:** Rust 2024, `clap`, `portable-pty`, standard library threading/synchronization, existing SSH tunnel launcher.

---

### Task 1: Refactor CLI Parsing To Support Explicit Modes

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/config.rs`
- Modify: `src/lib.rs`

**Step 1: Write the failing tests**

Add tests for:

- `hitch --port 38983`
- `hitch --port 38983 --origin 10.0.0.5 --user alice`
- rejecting `hitch --port 38983 -- aws login`
- rejecting invocations with neither `--port` nor `--`

Suggested test shape:

```rust
#[test]
fn parses_port_mode() {
    let config = Cli::try_parse_from(["hitch", "--port", "38983"])
        .unwrap()
        .into_config();

    assert_eq!(config.mode, Mode::Port { port: 38983 });
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- --nocapture`

Expected: FAIL because the config has no explicit mode yet.

**Step 3: Write minimal implementation**

Implement:

- `Mode` enum in `src/config.rs`
- `--port <port>` parsing in `src/cli.rs`
- mutual exclusion and mode selection validation

**Step 4: Run test to verify it passes**

Run: `cargo test --lib -- --nocapture`

Expected: PASS

**Step 5: Re-run the full test suite**

Run: `cargo test`

Expected: PASS

### Task 2: Add Port-Only Runtime Path

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/status.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

Add runtime tests for:

- starting one tunnel in port-only mode,
- returning failure when origin is missing,
- stopping the tunnel on simulated interrupt.

Suggested test shape:

```rust
#[test]
fn port_mode_starts_one_tunnel() {
    let launcher = FakeTunnelLauncher::new();
    let outcome = run_port_mode_for_test(launcher.clone(), Some("203.0.113.10".into()), None, 38983);

    assert_eq!(launcher.started_ports(), vec![38983]);
    assert_eq!(outcome.exit_code, 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- --nocapture`

Expected: FAIL because port-only runtime does not exist yet.

**Step 3: Write minimal implementation**

Implement:

- runtime dispatch based on `Mode`
- port-only tunnel startup path
- interrupt-aware shutdown for port mode
- status message for direct tunnel startup

**Step 4: Run test to verify it passes**

Run: `cargo test --lib -- --nocapture`

Expected: PASS

**Step 5: Re-run the full test suite**

Run: `cargo test`

Expected: PASS

### Task 3: Update Help Text And README

**Files:**
- Modify: `src/cli.rs`
- Modify: `README.md`

**Step 1: Write the failing tests**

Add tests for help text mentioning both invocation modes.

Suggested test shape:

```rust
#[test]
fn help_mentions_port_mode() {
    let mut command = Cli::command();
    let mut help = Vec::new();
    command.write_long_help(&mut help).unwrap();
    let help = String::from_utf8(help).unwrap();

    assert!(help.contains("--port <PORT>"));
    assert!(help.contains("hitch [OPTIONS] --port <PORT>"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- --nocapture`

Expected: FAIL until help text is updated.

**Step 3: Write minimal implementation**

Update:

- CLI help text and examples
- README usage and behavior documentation

**Step 4: Run test to verify it passes**

Run: `cargo test --lib -- --nocapture`

Expected: PASS

**Step 5: Final verification**

Run:

```bash
cargo test
env RUSTC_WRAPPER= timeout 5 cargo run -- --origin 127.0.0.1 --port 38983
```

Expected:

- all tests pass,
- Hitch prints that it is opening a reverse tunnel for port `38983`,
- the process remains running until interrupted or the timeout kills it.
