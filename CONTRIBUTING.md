# Contributing to Asterai
Thanks for your interest in contributing to Asterai!

## Getting Started
1. Fork the repository
2. Clone your fork locally
3. Run `cargo build` to verify everything compiles

## Code Style
Key points:

1. Keep functions small.
2. Avoid indentation hell: prefer to return early than to branch out.
3. Comments capitalise the first letter, and end with a full stop.
4. Comments go above the line, not at the end of it.
5. Avoid blank lines to separate concerns: create new functions instead,
   and comment logic by code e.g. function names rather than with section comments. 
6. `bool` vars should be named as questions, e.g. `is_set` instead of `set`.
   Functions returning `bool` should be worded as questions with a verb prefix,
   e.g. `check_is_set` or `get_is_set`.
7. Escape long strings with \ and a line break to keep them readable.
8. Prefer `match` over `if` for "ternary operations".

## Submitting Changes
1. Create a branch for your changes
2. Make your changes with clear commit messages
3. Run `cargo fmt` and `cargo clippy`
4. Open a pull request with a description of what you changed and why

## Questions?
Open an issue or join our Discord server at http://asterai.io/discord.
