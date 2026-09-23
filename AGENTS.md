# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Critical Rules

- Be extremely concise; sacrifice grammar for concision.
- Use built-in tools for file operations: globs for file search, grep
  for content search, read for viewing files.  Do not request
  grep/sed/fd/find/ls/cat or similar CLI tools when you already have
  these built-in.
- Read files completely before modifying them; use offset/limit to
  chunk large files.  Understand existing patterns and context before
  proposing changes.
- Always list unresolved questions at end.
- Keep documention (.md files) up to date with code changes.


# Revision Control

- Do not add Claude attribution to commit messages.
- Do not commit without permission.
- PRs should generally be comprised of one functional change; suggest
  making a commit before moving onto something unrelated.
- Commit and PR bodies should be concise and summarizing, not an
  enumeration of every change.  Default to a subject-only commit; add a
  short body only when needed.  When there are only one or two changes,
  the body may be specific about them.
- All tests must pass before committing.
- Never use -a to commit; always enumerate the files.


# Programming Rules

- Prefer ASCII in all code and user-facing strings (logs, CLI output,
  error messages).  Ask before using Unicode.
- Prefer consistency above most other concerns.
- Do not add trivial, obvious or redundant comments.
- Be DRY.
- Avoid magic constants.
- Only comment unintiutive or hard to understand code.
- Always comment data structures.
- Don't abbreviate by dropping letters from the middle of a word.
  Truncation (cutting from the end) is OK.   Domain acronyms are OK.


## Rust Rules

- Use the latest stable Rust edition.
- Always run `cargo +nightly fmt` after changes and before commits.
- Always run `cargo clippy` after changes and before commits; fix
  simple warnings, ask for guidance on complicated ones.
- Run tests with `cargo test`.
- Always use the narrowest visibility possible.
- Avoid public by default.
- Prefer `#[expect(lint, reason = "...")]` over `#[allow(lint)]`. If
  you must use `allow`, add `// [TODO] @<developer>: fix allow lint`.
- Use item-level imports, not nested crate/module imports.
- Prefer `use` statements at module top over inline imports.
- Avoid mutable variables when possible.  Prefer new bindings or shadows.
- Never re-export.  Use individual use statements where needed instead
  of `pub use` or `pub(crate) use`.
- Never employ a wildcard `use` statement on an enum when trying to
  shorten match arms.
- Use the newtype idiom as appropriate.
- Do not use `anyhow` in library crates at all; use typed errors via
  `thiserror` instead.  `anyhow` is for binaries only.
- Do not use `unsafe` without asking.
- When adding dependencies, use `cargo add` to ensure we install the
  latest version of dependencies.
