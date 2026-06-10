# VoxLine Agent Guide

VoxLine is a Linux-first, local-first dictation app written in Rust. Follow
`VoxLine_implementation_plan.md`, preserve privacy-safe defaults, and keep
changes focused.

## Workflow

Before implementing a change:

1. Sync the merged `main` branch and review the worktree. Commit existing
   intended untracked changes with a short, descriptive message. Never commit
   unrelated or sensitive files.
2. Read both the relevant GitHub issue and `VoxLine_implementation_plan.md`.
   The implementation document remains the source of truth when an issue is
   abbreviated.
3. Create a concise issue when one does not exist, add it to the private
   roadmap, and mark it In Progress.
4. Branch from `main`. Use a short name tied to the issue, such as
   `123-audio-capture`.
5. Implement and verify the complete milestone on that branch. Record hardware
   or external dependencies that prevent full validation.
6. Commit and push verified work. After the user confirms the issue is fixed,
   open a concise pull request to `main` that closes the issue.
7. Leave the roadmap item In Progress until the pull request merges; completed
   issues belong in Done.

Expose workspace development and manual validation commands as simple `just`
recipes. Keep the `justfile` current when adding or changing developer
workflows. Run `just check` before requesting user validation or opening a pull
request.

Do not block a milestone on explicitly deferred benchmarking or platform work.
Document the deferral in the issue or pull request and keep it tracked in the
appropriate later milestone.

## Writing

Keep READMEs, issues, pull requests, commit messages, and other agent-authored
documentation short and factual. Avoid emojis, promotional language, excessive
headings, boilerplate, and other common LLM writing patterns.
