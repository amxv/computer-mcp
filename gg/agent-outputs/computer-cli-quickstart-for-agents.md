# `computer` CLI Quickstart For Agents

This document is for an agent that has never seen this repo before.

## What `computer` Is

`computer` is a small client binary that you run **inside your own container**.

It connects to a **live VPS** that is already set up for you to do coding work.

That VPS is authenticated for the repository work you need help with and gives you three important remote coding actions:

- `exec-command`
  Runs a shell command on the remote VPS.
- `write-stdin`
  Continues or polls a running remote session.
- `apply-patch`
  Edits remote files using a structured patch.

This is meant to feel like Codex. The whole point is to simulate the core Codex environment on a live remote machine:

- run a command
- keep interacting with a terminal session
- edit files precisely

That is enough to inspect a repo, change code, run tests, and iterate on the VPS.

## The Main Idea

You do **not** SSH manually as your primary interface.

Instead:

1. download the `computer` client binary into your container
2. point it at the VPS URL and API key
3. use `exec-command`, `write-stdin`, and `apply-patch` to do your work

## Step 1: Download The Binary

Use `container.download` because that is your internet-enabled tool.

For a Linux x86_64 container, download this release asset:

```text
https://github.com/amxv/computer-mcp/releases/latest/download/computer-x86_64-unknown-linux-gnu
```

Save it somewhere simple, for example:

```text
/tmp/computer
```

Important:

- this is a single executable file
- it is **not** a tarball
- you do not need to extract it

## Step 2: Make It Executable

Use `container.exec`:

```bash
chmod +x /tmp/computer
```

Optional sanity check:

```bash
/tmp/computer --version
```

## Step 3: Authenticate Against The VPS

You need two values:

- the base URL of the remote server
- the API key

Example base URL:

```text
https://<host-or-runpod-proxy>
```

The CLI supports three ways to supply auth:

1. `--url` and `--key` on every command
2. `COMPUTER_URL` and `COMPUTER_KEY` environment variables
3. `computer connect` once, then reuse the saved profile

The simplest approach is to connect once:

```bash
/tmp/computer --url "https://<host>" --key "<api_key>" connect
```

After that, future `computer` commands can omit `--url` and `--key`.

## Step 4: Start Coding

At a high level, you will use these three commands.

### `exec-command`

Use this to run a remote shell command.

Mental model:

- “run this command on the VPS”

Useful arguments:

- the command string itself
- `--workdir` if you want to start in a specific remote directory
- `--yield-time-ms` if you want to wait longer before the command returns
- `--timeout-ms` if the command should be killed after a limit

Example:

```bash
/tmp/computer exec-command --workdir /workspace/repo "git status --short"
```

### `write-stdin`

Use this when a previous `exec-command` is still running and returned a `session_id`.

Mental model:

- “send more input to that remote process”
- or “poll that running session for more output”

Useful arguments:

- `--session-id`
- `--chars` to send input
- `--yield-time-ms` to wait for more output
- `--kill-process` if you want to terminate it

Example:

```bash
/tmp/computer write-stdin --session-id 42 --chars $'hello\n'
```

### `apply-patch`

Use this to edit files on the VPS.

Mental model:

- “apply this structured patch in that remote repo/directory”

Useful arguments:

- `--patch`
- `--workdir`

`--workdir` is required. Relative paths inside the patch resolve from that directory.

Example:

```bash
/tmp/computer apply-patch --workdir /workspace/repo --patch $'*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+new\n*** End Patch\n'
```

## The Normal Working Loop

Most tasks look like this:

1. inspect the remote repo with `exec-command`
2. change files with `apply-patch`
3. run tests or build commands with `exec-command`
4. if a process stays alive, continue it with `write-stdin`

That is the core remote coding loop.

## How To Publish A PR

To open a PR, use `exec-command` to run the remote command:

```bash
/tmp/computer exec-command --workdir /workspace/repo \
  "computer-mcp publish-pr --repo owner/repo --title 'Agent: change' --body 'Automated change.'"
```

That is the idea: once your changes are committed on the remote repo, run `computer-mcp publish-pr` through `exec-command`.

## Minimal First Commands

If you just want the shortest path from zero to useful:

1. download `computer-x86_64-unknown-linux-gnu` to `/tmp/computer`
2. run:

```bash
chmod +x /tmp/computer
```

3. authenticate once:

```bash
/tmp/computer --url "https://<host>" --key "<api_key>" connect
```

4. verify it works:

```bash
/tmp/computer exec-command "pwd"
```

5. start working:

- inspect with `exec-command`
- edit with `apply-patch`
- continue sessions with `write-stdin`

That is all you need to begin coding on the live VPS.
