# Security Policy

## Scope

ImGit is a desktop Git client. It runs `git` on your `PATH` for network operations, asks for HTTPS credentials in
its own sign-in bar and can hand them to your git credential helper, and renders content from the repositories you
open (commit messages, file contents, diffs). Problems in any of those areas are in scope, for example:

- credentials being logged, written to disk or sent anywhere other than the remote you asked for;
- a crafted repository, commit message, file name or diff that crashes ImGit or makes it run code;
- the built-in terminal, command palette or shell actions executing something you did not ask for;
- unsafe handling of the release packages (checksums, update instructions).

## Reporting a Vulnerability

Please do **not** open a public issue for a security problem. Report it privately to `orange@libomath.org` with the
ImGit version (or commit), your OS, and steps or a minimal repository that reproduces it.

You will get an acknowledgement, and the fix ships in the next release with credit to you unless you prefer to stay
anonymous. Please give us a reasonable time to fix the problem before disclosing it publicly.

## Supported Versions

Only the latest [release](https://github.com/alt-orange/ImGit/releases) and the `github-main` branch receive security
fixes.
