# Security Policy

`jtr` is a tool that fetches recipe manifests from a registry and writes them into users' project files. That puts it on the boundary of supply-chain risk, and we take security reports seriously.

## Reporting a vulnerability

**Please do not file public GitHub issues for security problems.** Instead, email **eb@erbiker.dev** with:

- A description of the issue and its potential impact.
- Steps to reproduce, ideally with a minimal proof-of-concept.
- The version of `jtr` you tested against (`jtr --version` or commit SHA).
- Any suggested mitigations you have in mind.

You should receive an acknowledgement within 72 hours. If you don't, please follow up — the message may have been missed.

We will work with you privately to confirm and fix the issue, then coordinate public disclosure (typically a CVE + a release note in [CHANGELOG.md](CHANGELOG.md)) once a patched version is available. Reporters are credited unless they prefer to remain anonymous.

## What we consider in-scope

- Anything in the `jtr` CLI binary that could let a malicious registry, malicious recipe, or man-in-the-middle attacker compromise a user's machine: arbitrary code execution, path traversal, unauthorized file writes, credential exfiltration, etc.
- Anything in the recipe-manifest format that could be abused to inject content into a user's `justfile` / `Taskfile.yml` beyond the documented managed-block region.
- Any failure of the integrity-checking mechanism (once checksum verification ships — see [issues tagged `security`](https://github.com/erbiker/jtr/issues?q=label%3Asecurity)).

## What is out of scope

- Vulnerabilities in third-party tools that `jtr` recipes shell out to (Docker, npm, cargo, etc.). Report those upstream.
- Issues that require the attacker to already have local write access to the user's project files or `~/.config/jtr/`.
- DoS attacks via oversized indexes or manifests, unless they are exploitable for something more serious.

## Supported versions

`jtr` is pre-1.0 today. Only the latest released version (and the `main` branch) receive security fixes. Once a stable 1.x release exists, we'll publish a support window here.
