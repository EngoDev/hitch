# Hitch Port Mode Design

**Date:** 2026-03-17

## Goal

Add a second invocation mode to Hitch so the user can open a reverse tunnel for a specific local port without wrapping a login command.

## User-Facing Behavior

Hitch will support two mutually exclusive modes:

```bash
hitch [--origin <host>] [--user <ssh-user>] -- <command> [args...]
hitch [--origin <host>] [--user <ssh-user>] --port <port>
```

Wrapped command mode keeps the current behavior: Hitch runs the command in a PTY, mirrors I/O, detects a localhost callback URL, and opens one reverse tunnel tied to the wrapped command lifetime.

Port-only mode skips command execution entirely. Hitch immediately opens a reverse tunnel for the requested port and keeps it running until the user presses `Ctrl+C`.

## Validation Rules

- `--port` cannot be combined with `--` or wrapped command arguments.
- Wrapped command mode still requires `--`.
- The user must select exactly one mode: `--port <port>` or `-- <command> ...`.
- `--port` must parse as a valid TCP port.

## Configuration Model

The config should move from an implicit wrapped-command shape to an explicit mode enum:

- `Mode::Command { command: Vec<String> }`
- `Mode::Port { port: u16 }`

Shared tunnel settings remain top-level configuration:

- `origin: Option<String>`
- `user: Option<String>`

This keeps mode-specific logic explicit and prevents ad hoc checks scattered through runtime code.

## Runtime Behavior

Wrapped command mode is unchanged.

Port-only mode should:

1. resolve origin and user,
2. format the SSH destination,
3. print that Hitch is opening a reverse tunnel for the requested port,
4. start the SSH reverse tunnel immediately,
5. wait until interrupted,
6. stop the tunnel cleanly on interrupt,
7. exit `0` on clean shutdown.

In this mode the tunnel is the whole purpose of the invocation, so failure to resolve origin or start the tunnel should produce a clear error and a non-zero exit code.

## Error Handling

Wrapped command mode keeps its current best-effort tunnel behavior.

Port-only mode is stricter:

- missing origin is a hard failure,
- tunnel launch failure is a hard failure,
- interrupt-driven shutdown is a successful exit path.

## Testing Strategy

Unit tests should cover:

- parsing valid port-only invocations,
- rejecting `--port` combined with wrapped command mode,
- rejecting invocations with no mode selected,
- preserving existing wrapped-command parsing.

Runtime tests should cover:

- port-only mode starting exactly one tunnel for the requested port,
- port-only mode failing when origin resolution is unavailable,
- port-only mode stopping the tunnel on simulated interrupt,
- wrapped command mode continuing to pass unchanged.
