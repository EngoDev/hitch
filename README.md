# Hitch

Hitch wraps login commands that expect an OAuth callback on `localhost` and makes that callback reachable when the command is running on a remote machine over SSH.

It is designed for flows like:

```bash
hitch -- aws sso login
hitch -- gh auth login
hitch --port 38983
```

When the wrapped command prints a loopback redirect URL such as `http://localhost:4567/callback`, Hitch:

1. keeps the wrapped command interactive by running it in a PTY,
2. mirrors the wrapped command's output to your terminal,
3. detects the first localhost callback URL with a port,
4. starts an SSH reverse tunnel for that port until the wrapped command exits.

## Usage

```bash
hitch [--origin <host>] [--user <ssh-user>] -- <command> [args...]
hitch [--origin <host>] [--user <ssh-user>] --port <port>
```

Examples:

```bash
hitch -- aws sso login
hitch --origin 203.0.113.10 --user alice -- gh auth login
hitch --origin 203.0.113.10 --port 38983
```

## Behavior

- Hitch only reacts to loopback redirect URLs: `localhost`, `127.0.0.1`, and `::1`.
- It only starts one tunnel per invocation. The first valid loopback URL wins.
- `--port <port>` opens a reverse tunnel immediately and keeps it alive until interrupted.
- If a redirect URL is not loopback, Hitch reports that the original login command configuration should be checked.
- If no origin can be determined, Hitch continues running the wrapped command and reports why tunneling could not be established.
- If the tunnel SSH session prompts for a password, Hitch pauses forwarding terminal input to the wrapped command until tunnel authentication completes.
- Hitch returns the wrapped command's exit status.

Origin resolution order:

1. `--origin`
2. the client IP from `SSH_CONNECTION`
3. no origin available, which disables tunneling for that run
