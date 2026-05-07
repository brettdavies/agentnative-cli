---
id: p8
title: Discoverable Through Agent Skill Bundles
last-revised: 2026-05-06
status: active
requirements:
  - id: p8-must-bundle-install
    level: must
    applicability:
      if: CLI ships an agent skill bundle
    summary: "When a skill bundle exists, the CLI provides an install path (`tool skill install [<host>]`) that registers the bundle with installed agent runtimes."
  - id: p8-should-bundle-exists
    level: should
    applicability: universal
    summary: "CLIs ship a top-level agent-discoverable markdown bundle (`AGENTS.md`, `SKILL.md`, or equivalent) with YAML frontmatter naming the tool and capability summary."
  - id: p8-may-install-all
    level: may
    applicability:
      if: CLI ships an agent skill bundle
    summary: "An `--all` mode auto-detects installed runtimes (Claude Code, Cursor, Codex, OpenCode, etc.) and installs across all."
  - id: p8-may-bundle-update
    level: may
    applicability:
      if: CLI ships an agent skill bundle
    summary: "An update/upgrade subcommand (`tool skill update`) pulls the latest bundle version."
---

# P8: Discoverable Through Agent Skill Bundles

## Definition

A skill bundle is a structured markdown file (canonical names: `AGENTS.md` or `SKILL.md`) with YAML frontmatter that
names the tool, describes its capabilities, and provides workflow guidance an agent can load into its runtime. The
bundle lives outside the CLI's flag space: agents discover it via filesystem convention, not via `--help`.

## Why Agents Need It

`--help` describes what is *possible* (the flag and subcommand surface); a skill bundle describes what to *do* (workflow
knowledge, common compositions, recovery patterns). Workflow knowledge does not fit in `after_help` examples. Without a
bundle, every invocation begins with a `--help` round-trip plus inference; with one, the agent loads `SKILL.md` once and
recognizes the tool's idioms across every subsequent invocation.

## Requirements

**MUST:**

- When a CLI ships a skill bundle, the CLI MUST provide an install path that registers the bundle with installed agent
  runtimes. The canonical form is a `tool skill install [<host>]` subcommand that writes into the runtime's filesystem
  cascade (e.g., `~/.claude/skills/`, `~/.cursor/skills/`). Non-canonical alternatives (`tool init --skill`, `tool
  skills add`, `tool agents add`) are acceptable but SHOULD migrate toward `tool skill install`. A bundle without an
  install path sits unread until a human manually copies it; the install path is what turns the bundle from
  documentation into discoverable runtime knowledge.

**SHOULD:**

- CLIs SHOULD ship a top-level agent-discoverable markdown bundle (canonical names are `AGENTS.md` or `SKILL.md`, both
  recognized by major agent runtimes) with YAML frontmatter naming the tool and summarizing its capabilities. The
  bundle's first job is to be findable by filesystem convention; its second is to teach the agent how to invoke the tool
  well.

**MAY:**

- An `--all` mode MAY auto-detect installed agent runtimes (Claude Code, Cursor, Codex, OpenCode, and others as the
  ecosystem evolves) and install the bundle across each. A user setting up a new machine with multiple coding agents
  installs once and gets coverage across every runtime.
- An `update` (or `upgrade`) subcommand under `tool skill` MAY pull the latest bundle version, so agents stay current
  with the CLI's evolving surface without a full reinstall.

## Evidence

- A top-level `AGENTS.md` or `SKILL.md` in the CLI's source tree (and shipped in the release artifact) with YAML
  frontmatter declaring at least the tool name and a one-line capability summary.
- A `skill` subcommand group in the CLI enum (e.g., `tool skill install`, `tool skill update`, `tool skill list`).
- An installer that targets the runtime cascade directly (file writes to `~/.claude/skills/<tool>/`, etc.) rather than
  requiring the runtime to be running.
- Bundle content versioned alongside the CLI's release: the bundle ships from the same commit as the binary, not from a
  separate doc tree that drifts.

## Anti-Patterns

- A CLI shipping a skill bundle with no install path: the bundle sits unread until a human manually copies it.
- An install path that requires the agent runtime to be running: `tool skill install` writes to the runtime's filesystem
  cascade (e.g., `~/.claude/skills/`) rather than requiring an active session.
- A bundle whose contents drift from the CLI's actual surface: the bundle is part of the CLI's release artifact, not a
  separate doc tree.

The vendor census in the v0.4.0 source-mining sprint documents the shipped patterns across Firecrawl, CLI-Anything, gws,
Crush, and larksuite; the `agentnative-skill` repo's `bin/check-update` is a reference for an update-check pattern.
