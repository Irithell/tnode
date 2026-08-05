# Changelog

## [0.1.1] - 2026-08-04

### Added
- `--tnode-update` — self-updater command with atomic file replacements
- `--tnode-changelog` — terminal panel for changelog viewing with colored status indicators
- Passive background update check (cached once every 24h, printed to stderr post-execution)
- Automatic changelog display on first run after upgrade

### Changed
- Refactored `VERSION` to use `env!("CARGO_PKG_VERSION")` from `Cargo.toml`
- Unified cache invalidation hashing in `libthook` to match `tnode` nanosecond precision

## [0.1.0] - 2026-08-04

### Added
- Initial release — `libthook.so` POSIX shim and `tnode` CLI bridge for Termux
- `realpath` opaque path translation for Node.js
- `execve` / `execvp` shebang rewrite and ELF Copier bypass
- `symlink` / `symlinkat` / `chmod` / `fchmodat` SDCARD workarounds
- `flock` / `fcntl` / `syscall` file locking shims
- `mkfifo` / `mkfifoat` FIFO redirection to `$TMPDIR`
- `dlopen` shared library cache bypass for `.node` native addons
- `bridge.toml` configuration support
