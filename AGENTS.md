# VoxLine Agent Guide

VoxLine is a Linux-first, local-first dictation app written in Rust. Follow
`VoxLine_implementation_plan.md`, preserve privacy-safe defaults, and keep
changes focused.

## Workflow

Before implementing a change:

1. Review the worktree. Commit existing intended untracked changes with a short,
   descriptive message. Never commit unrelated or sensitive files.
2. Create a concise GitHub issue describing the work and expected result.
3. Branch from `main`. Use a short name tied to the issue, such as
   `123-audio-capture`.
4. Implement and verify the change on that branch.
5. After the user confirms the issue is fixed, open a concise pull request from
   the branch to `main`.

## Writing

Keep READMEs, issues, pull requests, commit messages, and other agent-authored
documentation short and factual. Avoid emojis, promotional language, excessive
headings, boilerplate, and other common LLM writing patterns.
