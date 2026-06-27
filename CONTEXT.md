# Context

## Glossary

### Personal embedded-hal driver lab

A personal collection of embedded-hal Rust drivers kept primarily for the maintainer's own understanding, hardware use, and local reuse. The repository values simple source code, practical examples, and understandable behavior over publication polish or external contribution workflow.

### Full control

The ability to try lower-level device behavior when needed without modeling every datasheet feature as a high-level API up front. Control means preserving practical escape hatches for experimentation while keeping the main driver path simple and effective.

### Small focused commits

Commits that each capture one coherent change and use short conventional prefixes such as `fix:`, `test:`, `chore:`, or `docs:`. A focused commit should make sense on its own instead of mixing unrelated cleanup, feature work, and documentation changes.
