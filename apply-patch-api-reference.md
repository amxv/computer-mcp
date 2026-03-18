# `apply_patch` API Reference

This document describes the `apply_patch` tool exactly as I understand it from the tool contract I was given, plus the runtime behavior I directly observed while probing the tool in this repository.

It is written for someone implementing a Codex plugin or tool integration.

## Overview

`apply_patch` is a dedicated tool for editing files. It is not a shell command and it is not called via JSON.

The tool accepts one freeform string input containing a patch in a strict custom format. That patch can:

- create files
- update files
- delete files
- move or rename files
- modify multiple files in one invocation

The tool contract also included one operational constraint:

- Do not call `apply_patch` in parallel with other tools.

## Invocation Model

The tool is defined as a FREEFORM input tool.

That means:

- the input is raw text, not JSON
- the input must conform to the patch grammar
- the patch must start with `*** Begin Patch`
- the patch must end with `*** End Patch`
- there must be one or more hunks between those markers

## Formal Grammar From The Tool Contract

The exact grammar provided to me was:

```text
start: begin_patch hunk+ end_patch
begin_patch: "*** Begin Patch" LF
end_patch: "*** End Patch" LF?

hunk: add_hunk | delete_hunk | update_hunk
add_hunk: "*** Add File: " filename LF add_line+
delete_hunk: "*** Delete File: " filename LF
update_hunk: "*** Update File: " filename LF change_move? change?

filename: /(.+)/
add_line: "+" /(.*)/ LF -> line

change_move: "*** Move to: " filename LF
change: (change_context | change_line)+ eof_line?
change_context: ("@@" | "@@ " /(.+)/) LF
change_line: ("+" | "-" | " ") /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF
```

## Patch Envelope

Every valid patch must look like this at the top level:

```text
*** Begin Patch
... one or more hunks ...
*** End Patch
```

The tool requires at least one hunk.

## Supported Hunk Types

The contract supports four practical operations:

1. Add a file
2. Update a file
3. Delete a file
4. Move or rename a file, optionally with edits

## Add File Syntax

An add hunk creates a file.

```text
*** Add File: path/to/file.txt
+line one
+line two
+line three
```

Rules:

- Every content line in an `Add File` hunk must start with `+`.
- There is no `@@` section in a pure add hunk.
- The path follows `*** Add File: ` on the same line.

Example:

```text
*** Begin Patch
*** Add File: notes/todo.txt
+buy milk
+ship plugin
*** End Patch
```

## Update File Syntax

An update hunk edits an existing file.

```text
*** Update File: path/to/file.txt
@@
-old line
+new line
 unchanged context
```

Rules:

- `*** Update File: ...` identifies the file to edit.
- Update content is represented with one or more change blocks.
- Each change block begins with a context marker such as `@@`.
- Inside a change block:
  - `-` means remove this line
  - `+` means add this line
  - a leading space means context line retained as-is
- `*** End of File` is optional and may appear at the end of the change sequence.

Example:

```text
*** Begin Patch
*** Update File: src/app.js
@@
-console.log("old");
+console.log("new");
*** End Patch
```

## Delete File Syntax

A delete hunk removes a file.

```text
*** Delete File: path/to/file.txt
```

Example:

```text
*** Begin Patch
*** Delete File: tmp/old.txt
*** End Patch
```

## Move Or Rename Syntax

Moves are expressed as an update hunk with a `*** Move to: ...` line immediately after the update header.

```text
*** Update File: old/path/file.txt
*** Move to: new/path/file.txt
@@
-old content
+new content
```

Notes:

- The move line is optional.
- A move may be combined with content edits.
- Based on the grammar, the move line comes before the change section.

## Multiple Files In One Invocation

Yes, the tool supports multiple file operations in one `apply_patch` call.

The grammar allows `hunk+`, meaning one or more hunks can appear between `*** Begin Patch` and `*** End Patch`.

Example:

```text
*** Begin Patch
*** Update File: src/app.js
@@
-console.log("old");
+console.log("new");

*** Add File: src/utils.js
+export function add(a, b) {
+  return a + b;
+}

*** Delete File: src/unused.js
*** End Patch
```

This is the main way to do multi-file editing, creation, and deletion in one shot.

## Input Shape Summary

### Successful Input Shape: Add File

```text
*** Begin Patch
*** Add File: <path>
+<line 1>
+<line 2>
*** End Patch
```

### Successful Input Shape: Update File

```text
*** Begin Patch
*** Update File: <path>
@@
-<line to remove>
+<line to add>
 <context line>
*** End Patch
```

### Successful Input Shape: Delete File

```text
*** Begin Patch
*** Delete File: <path>
*** End Patch
```

### Successful Input Shape: Move Or Rename

```text
*** Begin Patch
*** Update File: <old path>
*** Move to: <new path>
@@
-<old line>
+<new line>
*** End Patch
```

### Successful Input Shape: Multi-File Patch

```text
*** Begin Patch
*** Update File: <path-a>
@@
-<old>
+<new>

*** Add File: <path-b>
+<content>

*** Delete File: <path-c>
*** End Patch
```

## Runtime Success Output Shapes Observed

I directly observed these success outputs while using the tool.

### Add Success

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
A /absolute/path/file.txt
```

### Modify Success

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
M /absolute/path/file.txt
```

### Delete Success

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
D /absolute/path/file.txt
```

### Multi-File Success

For multi-file changes, the same shape is used, with one status line per affected file:

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
M /absolute/path/file-a.txt
A /absolute/path/file-b.txt
D /absolute/path/file-c.txt
```

Observed status letters:

- `A` for added file
- `M` for modified file
- `D` for deleted file

## Runtime Failure Output Shapes Observed

I directly reproduced and captured these failures.

### Failure: Update Context Did Not Match

This happens when an `Update File` hunk references lines that do not exist in the target file.

Observed output:

```text
apply_patch verification failed: Failed to find expected lines in /absolute/path/file.txt:
this line does not exist
```

Interpretation:

- the patch syntax was accepted
- the tool could not apply the update because the expected removed/context lines did not match the current file contents

### Failure: Update Missing File

This happens when an `Update File` hunk targets a file that does not exist.

Observed output:

```text
apply_patch verification failed: Failed to read file to update /absolute/path/missing.txt: No such file or directory (os error 2)
```

Interpretation:

- the file could not be opened for update
- the error includes the path and the OS-level reason

### Failure: Delete Missing File

This happens when a `Delete File` hunk targets a file that does not exist.

Observed output:

```text
apply_patch verification failed: Failed to read /absolute/path/missing.txt: No such file or directory (os error 2)
```

Interpretation:

- delete failed before modification because the file could not be read

## Failure Input Shapes

### Failure Input Shape: Update Context Mismatch

```text
*** Begin Patch
*** Update File: <existing path>
@@
-line that is not actually present
+replacement
*** End Patch
```

Typical output:

```text
apply_patch verification failed: Failed to find expected lines in <path>:
line that is not actually present
```

### Failure Input Shape: Update Missing File

```text
*** Begin Patch
*** Update File: <missing path>
@@
+new line
*** End Patch
```

Typical output:

```text
apply_patch verification failed: Failed to read file to update <path>: No such file or directory (os error 2)
```

### Failure Input Shape: Delete Missing File

```text
*** Begin Patch
*** Delete File: <missing path>
*** End Patch
```

Typical output:

```text
apply_patch verification failed: Failed to read <path>: No such file or directory (os error 2)
```

## Likely Error Categories

Based on the contract and observed behavior, these are the main error categories an integration should expect.

### 1. Verification Failure: Context Mismatch

Meaning:

- the patch targeted a real file
- the requested removal/context lines did not match current contents

Observed wording:

```text
apply_patch verification failed: Failed to find expected lines in <path>:
<line excerpt>
```

### 2. Verification Failure: Missing File On Update

Meaning:

- `Update File` targeted a path that does not exist

Observed wording:

```text
apply_patch verification failed: Failed to read file to update <path>: No such file or directory (os error 2)
```

### 3. Verification Failure: Missing File On Delete

Meaning:

- `Delete File` targeted a path that does not exist

Observed wording:

```text
apply_patch verification failed: Failed to read <path>: No such file or directory (os error 2)
```

### 4. Parse Or Grammar Failure

The contract strongly implies parse failures are possible if the input does not match the required grammar.

Examples of likely causes:

- missing `*** Begin Patch`
- missing `*** End Patch`
- invalid hunk header
- invalid line type inside a hunk
- malformed `@@` change block in an update

Important note:

- I did not successfully capture a literal parser-error payload during this session.
- So I can say this failure class should exist, but I cannot provide a confirmed exact error string for it from runtime observation.

### 5. File Operation Failure

Likely causes:

- unreadable file
- path problems
- filesystem errors

Observed examples surfaced as verification failures with OS error details.

## What An Integration Should Assume

For plugin design, the safest assumptions are:

- input is a single raw string, not JSON
- output is plain text, not JSON
- success output includes a short status block
- failure output may be a single error string
- paths in outputs may be absolute
- multiple file operations may succeed in one invocation
- failures can happen at parse time, verification time, or filesystem-read time

## Recommended Output Handling

An integration should parse responses defensively.

Recommended approach:

1. Treat the call as successful if the tool invocation itself returns success.
2. Extract affected files from the `Success. Updated the following files:` block when present.
3. Preserve the raw error text on failure.
4. Specifically detect and classify:
   - `Failed to find expected lines`
   - `Failed to read file to update`
   - `Failed to read`
   - any future parse-error prefixes

## Practical Examples

### Example: Create Two Files And Delete One

```text
*** Begin Patch
*** Add File: docs/a.txt
+hello

*** Add File: docs/b.txt
+world

*** Delete File: docs/old.txt
*** End Patch
```

Expected success shape:

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
A /absolute/path/docs/a.txt
A /absolute/path/docs/b.txt
D /absolute/path/docs/old.txt
```

### Example: Update Two Files In One Call

```text
*** Begin Patch
*** Update File: src/a.js
@@
-const a = 1;
+const a = 2;

*** Update File: src/b.js
@@
-const b = 1;
+const b = 2;
*** End Patch
```

Expected success shape:

```text
Exit code: 0
Wall time: 0 seconds
Output:
Success. Updated the following files:
M /absolute/path/src/a.js
M /absolute/path/src/b.js
```

### Example: Failing Update Due To Wrong Context

```text
*** Begin Patch
*** Update File: src/a.js
@@
-line that is absent
+replacement
*** End Patch
```

Expected failure shape:

```text
apply_patch verification failed: Failed to find expected lines in /absolute/path/src/a.js:
line that is absent
```

## Known Limits Of This Reference

This document combines:

- the formal grammar from the tool instructions
- direct runtime observations from this session

What is confirmed:

- grammar envelope and hunk types
- multi-file support
- success output structure
- several verification failure messages

What is not confirmed by direct observation here:

- the exact literal text of a parser/grammar failure
- the exact literal text for every possible filesystem or permission error
- whether all environments expose identical success headers

## Bottom Line

`apply_patch` is a structured freeform patch tool with:

- a strict patch envelope
- support for add, update, delete, and move operations
- support for multiple file hunks in one call
- plain-text success responses listing changed files
- plain-text failure responses describing verification or file-read problems

For a plugin integration, model it as:

- input: one raw patch string
- output: plain-text status or error text
- execution style: atomic tool invocation, not shell piping
*** End Patch
天天中彩票 to=functions.exec_command  大发彩票官网  北京赛车前આjson
