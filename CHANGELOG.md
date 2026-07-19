# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Nothing yet!

## [0.1.0] - 2026-07-19

### Added

- `taco {name}` runs one of your commands; a bare `taco` opens a full-screen fuzzy picker over everything that is available (the runnable builtins included)
- `taco add` / `taco edit` / `taco rm` manage your commands, in your editor or inline
- `taco ls` shows a flat list of the resolved commands; `taco ls --verbose` shows a tree mirroring the resolution order, with shadowed definitions greyed out
- `taco which {name}` shows what would run and where it is defined, including the definitions it shadows
- `taco alias` / `taco unalias` attach reusable preset projects (like `vitest` or `rust`) to a directory
- `taco config` opens the config in your editor, validates it on close, and offers to restore the previous version when an edit broke it
- `taco doctor` finds stale projects, dead aliases, and aliases already provided by a parent directory — `--fix` cleans them up
- `taco completions` generates directory-aware tab completions for zsh, bash, and fish

[unreleased]: https://github.com/RobinMalfait/taco/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/RobinMalfait/taco/releases/tag/v0.1.0
