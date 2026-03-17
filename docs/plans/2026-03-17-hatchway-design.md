# Hatchway Design

**Date:** 2026-03-17

## Goal

Build a Rust CLI wrapper that runs an arbitrary login command on a remote machine, preserves the command's interactive terminal behavior, detects the first loopback OAuth redirect URL printed by that command, and starts an SSH reverse tunnel for the callback port until the wrapped command exits.

## User-Facing Behavior

Hatchway is invoked as a wrapper around an existing login command:

```bash
hatchway [--origin <host>] [--user <ssh-user>] -- <command> [args...]
```

The wrapped command runs as if it were launched directly in the terminal. Hatchway mirrors the wrapped command's output to the screen and forwards terminal input back to the wrapped process so interactive CLIs continue to work.

While the wrapped command is running, Hatchway scans streamed output for URLs. The first detected URL whose host is `localhost`, `127.0.0.1`, or `::1` and that includes an explicit port is treated as the callback URL. Hatchway prints a status message that it found the URL, shows the parsed host and port, shows where it will tunnel to, and starts an SSH reverse tunnel:

```text
ssh -N -R <port>:localhost:<port> <user>@<origin>
```

The tunnel remains active until the wrapped command exits. Hatchway then terminates the tunnel process, waits for cleanup, and exits with the wrapped command's exit status.

## Scope Boundaries

Hatchway does not rewrite URLs, proxy HTTP traffic, or inspect callback requests. It trusts that the wrapped login command prints the correct URL and usage instructions for the user. Hatchway only detects eligible loopback redirect URLs and makes their callback port reachable from the user's local machine through SSH reverse port forwarding.

Hatchway manages at most one tunnel per invocation. If multiple URLs are printed, the first valid loopback URL wins. Repeated URLs do not start additional tunnels.

## CLI Semantics

Hatchway-owned options:

- `--origin <host>`: override the SSH tunnel origin host or IP.
- `--user <ssh-user>`: override the SSH user used for the reverse tunnel.

Everything after `--` is forwarded verbatim as the wrapped login command.

Origin resolution:

1. Use `--origin` if provided.
2. Otherwise parse `SSH_CONNECTION` and take the client IP from the first field.
3. Otherwise report that tunneling could not be established because no origin could be determined.

User resolution:

1. Use `--user` if provided.
2. Otherwise omit the username and let `ssh` apply its default user resolution.

## Architecture

The implementation should separate into a thin orchestration layer and pure helper modules.

The orchestration layer is responsible for:

- launching the wrapped command in a PTY,
- mirroring PTY output to stdout,
- forwarding stdin to the PTY,
- feeding output chunks into the URL detector,
- launching the SSH tunnel once,
- shutting the tunnel down when the wrapped command exits,
- returning the wrapped command's exit code.

Helper modules are responsible for:

- command-line parsing,
- origin and user resolution,
- URL extraction from arbitrary output text,
- loopback host validation,
- callback port extraction,
- SSH destination formatting,
- structured status messaging.

This split keeps platform-sensitive PTY code small and makes the core behavior unit-testable.

## URL Detection Rules

Detection is streaming and first-match only. Hatchway scans printed output as text arrives and looks for URL candidates. Each candidate is parsed and classified:

- If the host is loopback and a port is present, Hatchway starts the tunnel once.
- If the host is not loopback, Hatchway prints that the redirect URL does not lead to localhost and that the user should check the original login command configuration.
- If the URL is loopback but has no port, Hatchway prints that tunneling cannot be established because no callback port was found.

To avoid repeated side effects, Hatchway should track whether it already:

- started a tunnel,
- warned about an invalid redirect URL,
- reported a missing origin.

## Error Handling

Tunneling is best-effort support around the wrapped command. Hatchway should continue running the wrapped command and mirroring output even when tunnel setup fails.

Expected tunnel-related failure cases:

- `SSH_CONNECTION` missing and no `--origin` provided,
- `ssh` executable unavailable,
- non-loopback redirect URL detected,
- loopback URL without an explicit port,
- SSH tunnel process fails to start.

Each case should produce a concise status message describing what was detected and why tunneling was not established.

The only time Hatchway should fail independently is when it cannot start the wrapped command at all. Otherwise it returns the wrapped command's exit status.

## Testing Strategy

Most tests should target pure logic, not PTY internals.

Unit tests:

- parse CLI arguments into Hatchway options plus wrapped command,
- resolve origin from CLI flags and `SSH_CONNECTION`,
- resolve SSH destination string with and without explicit user,
- extract URL candidates from streamed output,
- validate loopback hosts,
- extract callback ports,
- enforce first valid URL wins,
- avoid duplicate tunnel launches.

Integration-style tests:

- fake a wrapped command output stream and verify tunnel launch happens once for the first valid loopback URL,
- verify non-loopback URLs produce the expected warning and no tunnel launch,
- verify tunnel teardown is triggered when the wrapped command exits.

The PTY boundary should be abstracted enough that orchestration can be tested with fakes instead of requiring a real interactive session in most tests.
