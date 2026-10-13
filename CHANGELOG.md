# Changelog

## [0.1.5] - 2026-10-12

### Added
- Direct execution support for shell scripts (`tnode start.sh`, `tnode ./start.sh`) with intelligent interpreter discovery and shell fallback
- Interactive update prompt (Oh-My-Zsh style) when updates are available on interactive TTY sessions, with 24-hour deferral on skip
- True cross-process mutual exclusion for `flock(2)` on SDCARD by emulating locks via POSIX `fcntl(F_SETLK/F_SETLKW)`
- Centralized `is_sdcard_path` detection using prefix boundaries and symlink canonicalization, eliminating false positives from internal paths
- Android system property inspection (`ro.build.selinux`, `ro.boot.selinux`) for `--tnode-doctor` under sandboxed app domains

### Changed
- Preserved `envp` environment in `execve` shebang rewrites, eliminating child process environment loss
- Resolved `#!/usr/bin/env <cmd>` directly to `$PREFIX/bin/<cmd>`, resolving `env: "node: No such file or directory"` failures
- Guaranteed `$PREFIX/bin` presence in child process `PATH`
- Refactored `--tnode-doctor` tool validation: replaced arbitrary essential tools list with Node.js runtime and configured project aliases
- Fixed dangling SELinux status output in `--tnode-doctor`
- Deferred string allocations in `libthook` `execve`/`execvp` until failure path, guaranteeing post-fork async-signal safety

## [0.1.4] - 2026-08-08

### Added
- 32-bit Android architecture support for `armv7-linux-androideabi` (`armv7`) and `i686-linux-android` (`i686`)
- Architecture detection in `install.sh` and `--tnode-update` for 32-bit `armv7` and `i686` targets
- Generic pure-Rust atomic pointer fallback for 32-bit `dlopen` symbol interception

### Changed
- Replaced hardcoded syscall number `73` with portable `libc::SYS_flock` for cross-architecture 32-bit/64-bit support

## [0.1.3] - 2026-08-06

### Added
- `--tnode-stats` — cache usage report grouped by file type (ELF copies, dlopen cache, pipe redirects)
- `--tnode-completions <bash|zsh|fish>` — shell completion script generator (Bash, Zsh, Fish)
- `--tnode-clean-max <MB>` — remove cache files exceeding a configurable size threshold
- `--tnode-hook-log <hooks>` — force trace-level logs for specific hooks without enabling global trace mode (e.g. `dlopen,execve`)
- `[hooks]` section in `bridge.toml` — per-project hook enable/disable with hook name validation
- `[core] hook_log` in `bridge.toml` — persistent per-project selective hook logging
- `[core] root = true` in `bridge.toml` — mark a directory as the root for upward bridge.toml discovery
- `bridge.toml` ascending directory discovery — walks parent directories up to `$HOME` or `/sdcard` root
- `--tnode-doctor` enhancements — direct `$PATH` lookup (no `which` dependency), Android API level detection, TMPDIR free disk space check via `statvfs(2)`, and SELinux status check
- Runtime environment variables — `THOOK_DISABLE_HOOKS` and `THOOK_LOG_HOOKS`

### Changed
- Replaced `which` tool dependency in `handle_doctor` and `generate_default_config` with direct `$PATH` executable lookup

## [0.1.2] - 2026-08-05

### Added
- Path suitability validation for custom temporary directory (`tmp`) and library (`libthook.so`) configs
- Automatic fallback with warning logs when configured `tmp` is unwritable or on a noexec SDCARD partition

### Changed
- Refactored `print_changelog` to default to the latest version (`latest`) and support `all` or specific version queries
- Trigger changelog panel display immediately upon completing `--tnode-update`

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
