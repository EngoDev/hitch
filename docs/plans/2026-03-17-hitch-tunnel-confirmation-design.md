# Hitch Tunnel Confirmation Design

**Date:** 2026-03-17

## Goal

Add a confirmation and edit flow before Hitch opens an auto-detected reverse tunnel in wrapped-command mode.

## Scope

This flow applies only when Hitch auto-detects tunnel details from wrapped-command output. It does not apply to direct `--port` mode.

## User-Facing Behavior

When Hitch detects a localhost callback URL and resolves the tunnel destination inputs, it should print the details it intends to use:

- callback port
- SSH user
- origin host or IP

Then it should ask the user, via `inquire`, whether those details look correct.

If the user confirms, Hitch opens the tunnel with the detected values.

If the user says the details are not correct, Hitch enters an edit flow in this exact order:

1. port
2. user
3. origin

Each prompt is prefilled with the current detected value. After the edit flow completes, Hitch proceeds with the edited values.

For the first version, the interaction is single-pass:

- show detected values,
- ask for confirmation,
- if needed, run one edit pass,
- launch with the resulting values.

No repeated confirmation loop is required.

## Validation

- `port` must parse as a valid `u16`
- `user` may be empty, which means no explicit SSH user should be included in the destination
- `origin` must be non-empty

## Architecture

The detector remains pure and unchanged in responsibility. It only signals that a valid loopback callback was found.

The tunnel launch path becomes:

1. build a tunnel configuration from detected values,
2. print the detected details,
3. run a confirmation/editor abstraction,
4. launch the tunnel with the returned values.

This keeps output detection separate from interactive correction.

## Testing Strategy

Tests should cover:

- accepting the detected values without edits,
- rejecting the detected values and applying edited `port`, `user`, and `origin`,
- clearing the explicit user by submitting an empty string,
- rejecting invalid edited port input,
- confirming that `--port` mode bypasses the confirmation/edit flow.
