---
name: web-design-guidelines
description: Review UI code for Web Interface Guidelines compliance. Use when asked to "review my UI", "check accessibility", "audit design", "review UX", or "check my site against best practices".
metadata:
  author: vercel
  version: "1.0.0"
  argument-hint: <file-or-pattern>
---

# Web Interface Guidelines

Review files for compliance with Web Interface Guidelines.

## How It Works

1. Read the guidelines from `command.md` in this skill's directory
2. Read the specified files (or prompt user for files/pattern)
3. Check against all rules in the guidelines
4. Output findings in the terse `file:line` format

## Guidelines Source

The guidelines are vendored next to this file as [`command.md`](command.md)
(from vercel-labs/web-interface-guidelines, MIT; see `LICENSE`). Read that
local file. Do not fetch it from the network: a remote copy could change
what this skill does without review.

## Usage

When a user provides a file or pattern argument:
1. Read `command.md` in this skill's directory
2. Read the specified files
3. Apply all rules from the guidelines
4. Output findings using the format specified in the guidelines

If no files specified, ask the user which files to review.
