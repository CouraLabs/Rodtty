# Issue tracker: GitHub

Issues and specs live in CouraLabs/Rodtty's GitHub Issues.
Use the `gh` CLI from this repository.

## Conventions

- Verify the target before creating issues:
  `gh repo view --json nameWithOwner`.
- Create: `gh issue create --title "..." --body-file <file>`.
  Use a temporary UTF-8 file for multiline bodies.
- Read: `gh issue view <number> --json number,title,body,labels,comments`.
- List: `gh issue list --state open --json number,title,labels`.
- Comment: `gh issue comment <number> --body-file <file>`.
- Label: `gh issue edit <number> --add-label "..."` or
  `--remove-label "..."`.
- Close: post the explanation as a comment, then run
  `gh issue close <number>`.
- Link related work using GitHub sub-issues and native issue
  dependencies where supported. Otherwise use `Part of #<parent>`
  and `Blocked by: #<number>` at the top of the issue body.
- Work is unblocked when every blocker is closed.

File issues only in CouraLabs/Rodtty.

## Pull requests as a triage surface

**PRs as a request surface: no.**

## Skill operations

When a skill says "publish to the issue tracker", create a GitHub issue.
When it says "fetch the relevant ticket", read the GitHub issue.
