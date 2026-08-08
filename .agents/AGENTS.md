# thook / tnode — Agent Reference

This document is the authoritative technical reference for an AI agent working
on this project. Read it in full before making any change to the codebase.

---

## Overview

The project is a POSIX compatibility shim and Node.js bridge for Android
Termux. It is implemented as two Rust crates compiled for Android Bionic
(`aarch64-linux-android` / `x86_64-linux-android` / `armv7-linux-androideabi` / `i686-linux-android`):

- **`thook`** (`crates/thook/src/lib.rs`): a `cdylib` that compiles to
  `libthook.so`. It is injected into any process via `LD_PRELOAD` and
  intercepts libc calls to apply Android SDCARD workarounds.
- **`tnode`** (`crates/tnode/src/main.rs`): a binary CLI wrapper. It finds
  `libthook.so`, builds the correct environment, and `exec`s any command with
  the library pre-loaded.

The project targets **Termux on Android exclusively**. macOS (DYLD), Windows,
and desktop Linux are out of scope.

---

## Repository layout

```
.
├── Cargo.toml              Workspace (members: thook, tnode)
├── Cargo.lock
├── .cargo/config.toml      NDK linker paths — local only, NOT committed
├── .gitignore              Ignores dist/, target/, .cargo/
├── install.sh              End-user one-line installer
├── .agents/
│   └── AGENTS.md           This file
└── crates/
    ├── thook/
    │   ├── Cargo.toml      crate-type = ["cdylib"]
    │   ├── build.rs        Passes --version-script to the linker
    │   ├── export.map      Limits exported symbols to the intercepted set
    │   └── src/lib.rs      All hook logic
    └── tnode/
        ├── Cargo.toml
        └── src/main.rs     CLI entry point + support functions
```

---

## Build system

### Cross-compilation requirement

The crates target Android Bionic. They **cannot** be compiled with a standard
Linux glibc toolchain. Each developer must configure `.cargo/config.toml`
locally with the NDK `clang` linker paths:

```toml
[target.aarch64-linux-android]
linker = "/path/to/ndk/<ver>/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"

[target.x86_64-linux-android]
linker = "/path/to/ndk/<ver>/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android24-clang"
```

This file is listed in `.gitignore` and must never be committed.

### Release profile

`Cargo.toml` (workspace root) configures `[profile.release]` with:
- `strip = true` — removes symbols to reduce binary size.
- `opt-level = "z"` — optimizes for size.
- `lto = true` — enables link-time optimization.
- `codegen-units = 1` — maximizes optimization quality.
- `panic = "abort"` — removes panic unwinding overhead.

### Symbol visibility

`crates/thook/export.map` is a GNU version script that exports only the
symbols that must be visible to the dynamic linker for interposition:
`realpath`, `symlink`, `symlinkat`, `readlink`, `readlinkat`, `fchmodat`,
`execve`, `execvp`, `dlopen`. All other symbols are local. This prevents
namespace pollution and reduces `.so` size.

The `dlopen` entry in the export map is the assembly trampoline, not
`thook_dlopen_rust` directly. `thook_dlopen_rust` must remain local.

---

## libthook architecture

### Initialization (`#[ctor]` fn `init`)

The `init` function is tagged `#[ctor(unsafe)]` which causes it to run as a
shared library constructor (placed in `.init_array`) immediately after the
dynamic linker maps `libthook.so` into the process, before `main`. It:

1. Calls `dlsym(-1isize as *mut c_void, symbol_name)` for every intercepted
   libc function. The `-1isize` sentinel is Bionic's `RTLD_NEXT`. The
   resulting pointers are stored in `OnceLock<FnType>` statics.
2. Reads `THOOK_DEBUG_LEVEL` (`error`/`warn`/`info`/`trace`) or the legacy
   `THOOK_DEBUG=1` and stores the numeric level in `LOG_LEVEL: AtomicI32`.
3. Opens `THOOK_LOG_FILE` with `O_WRONLY|O_CREAT|O_APPEND` and stores the fd
   in `LOG_FD: AtomicI32`. `-1` means stderr.
4. Reads `/proc/self/exe` to determine whether the host process is Node.js
   and stores the result in `IS_NODE: OnceLock<bool>`.

### Logging

Logging is done via `libc::write` directly to avoid stdio re-entrancy. The
four severity macros (`log_error!`, `log_warn!`, `log_info!`, `log_trace!`)
check `LOG_LEVEL` atomically before formatting to avoid any allocation on the
hot path when logging is disabled.

### Hook pattern

Every exported function follows the same pattern:

```rust
#[no_mangle]
pub unsafe extern "C" fn <name>(<args>) -> <ret> {
    let orig = match ORIGINAL_<NAME>.get() {
        Some(f) => f,
        None => return <error_value>,
    };
    let res = orig(<args>);
    // intercept logic if res indicates failure
    res
}
```

The original function pointer is always tried first. The hook only acts when
the original call fails with a specific errno that indicates an SDCARD
restriction.

### dlopen trampoline (AArch64 assembly)

`dlopen` cannot be hooked with a simple Rust `#[no_mangle]` function because
the hook must sometimes fall through to the real `dlopen` without going
through a second Rust function frame — the original `filename` and `flags`
arguments must be passed in registers `x0`/`x1` exactly as the ABI requires.

The solution is an AArch64 assembly trampoline defined with `global_asm!`:

1. The trampoline saves `x0`/`x1` (filename, flags) on the stack.
2. It allocates a byte on the stack at `[sp+32]` initialized to `0`
   (`call_orig` flag).
3. It calls `thook_dlopen_rust(filename, flags, &call_orig)`.
4. If `call_orig == 0` the Rust function returned a result directly; the
   trampoline returns it.
5. If `call_orig != 0` the trampoline restores `x0`/`x1` from the stack and
   tail-jumps to the real `dlopen` pointer via `ORIG_DLOPEN_PTR`.

`ORIG_DLOPEN_PTR` is a `.data` symbol initialized in `init` via
`set_orig_dlopen_ptr`. This avoids any Rust global static that could be
subject to initialization ordering issues.

---

## Hook catalogue

### `realpath`

**Strategy: opaque (Node only)**

After calling the real `realpath`, if `IS_NODE` is true and the resolved path
starts with the Termux prefix but the input path does not, the hook returns
the absolute form of the input path instead of the resolved canonical path.
This prevents Node.js from discovering that SDCARD symlinks point into the
Termux prefix.

**Memory**: When `resolved_path` is `NULL`, the caller expects a
`malloc`-allocated result. The hook frees the libc-allocated result and
returns a new `malloc_cstring` buffer. Do not use `CString::into_raw` here;
Bionic's allocator and Rust's allocator may differ and `free()` from C would
corrupt the heap.

**Guard**: The opaque logic is gated on `IS_NODE`. Compilers (`clang`, `make`,
`python`) always see the real filesystem paths.

### `symlink` / `symlinkat`

Attempts the real symlink. On `EACCES`/`EPERM` on a SDCARD path:
- If the link path is inside a `.bin` directory: writes a POSIX shell wrapper
  script (`#!/bin/sh\nbasedir=$(dirname "$0")\nexec "$basedir/TARGET" "$@"`).
- Otherwise: performs a recursive physical copy of the target.
Returns 0 in both cases to report success.

### `chmod` / `fchmodat`

On `EACCES`/`EPERM` on a SDCARD path: returns 0 silently. The SDCARD FUSE
layer does not support permission bits.

### `flock` / `fcntl` / `syscall`

On any locking-related error (`ENOSYS`, `EACCES`, `EPERM`, `EBADF`) for an
fd whose `/proc/self/fd/<fd>` symlink resolves into `/storage/` or `/sdcard/`:
returns 0. This simulates a successful lock acquisition.

`syscall` intercepts `SYS_fcntl` and `SYS_flock` (number 73 on aarch64) for
runtimes that call locking through the libc `syscall()` wrapper.

### `execve` / `execvp`

On `EACCES` (noexec SDCARD):

1. **Shebang bypass**: reads up to 256 bytes of the target file. If it starts
   with `#!`, reconstructs `argv` with the interpreter at position 0 and the
   original filename inserted before the user's arguments. Interpreter paths
   `/bin/sh`, `/bin/bash`, `/usr/bin/env` are rewritten to their Termux
   equivalents (`$PREFIX/bin/sh`, etc.) to preserve `LD_PRELOAD`.
2. **ELF Copier bypass**: checks for the `\x7fELF` magic. If the file is an
   ELF binary, copies it to `$TMPDIR/thook_exec_<hash>_<name>` and re-executes
   from there.

**esbuild special case (inside ELF Copier)**:
- esbuild is a Go binary that calls `SYS_getcwd` directly. Android blocks
  reads on `/storage/emulated` at depth, making `os.Getwd()` fail.
- When the copied binary name contains "esbuild": all `argv` arguments that
  are relative paths (not starting with `-` or `/`) are prepended with the
  absolute CWD. `--outfile=` and `--outdir=` flags are also made absolute.
- Immediately before `execve`, the process CWD is changed to
  `/data/data/com.termux/files/usr/tmp` if the current CWD is on the SDCARD.
- **Do not** add a `getcwd` interceptor in `libthook.so` to handle this or
  similar Go issues; it breaks C/Python CWD resolution and breaks `node-gyp`
  builds (e.g., `bcrypt`).

### `dlopen` (via trampoline + `thook_dlopen_rust`)

On a SDCARD path: copies the file to `$TMPDIR/thook_dl_<hash>_<name>` and
calls the real `dlopen` with the tmp path. The same mtime+size hash is used
for cache invalidation.

System paths (`/system/`, `/apex/`, `/vendor/`) and known system libraries
(`libc.so`, `libz.so`, `libm.so`, `libdl.so`) pass through directly via the
`call_orig = 1` mechanism.

### `mkfifo` / `mkfifoat`

On `EACCES`/`EPERM`/`ENOSYS` on a SDCARD path: creates the FIFO at
`$TMPDIR/thook_fifo_<hash>_<name>` instead.

### `open` / `openat` / `open64` / `openat64`

On `ENOENT`: checks whether the path has a FIFO counterpart in `$TMPDIR` and
retries the open there. This makes named pipes created via the `mkfifo` hook
transparently accessible.

### `stat` / `lstat` / `stat64` / `lstat64`

Same FIFO-redirect logic: on `ENOENT`, tries the `$TMPDIR` counterpart.

### `link` / `linkat`

On `EACCES`/`EPERM`/`ENOSYS` for a new path on the SDCARD: falls back to a
recursive physical copy. Hard link identity semantics are lost. The hook logs
a `WARN`-level message to make this visible.

### `unlink` / `unlinkat`

After the primary unlink, silently attempts to unlink the `$TMPDIR` FIFO
counterpart (if any) to prevent orphaned pipe files.

### `readlink` / `readlinkat`

Currently transparent pass-throughs. They exist as future intercept points.

---

## tnode architecture

`tnode` is a standard Rust binary. Its `main` function:

1. Parses `--tnode-*` flags from `argv`, stopping at the first argument that
   does not start with `--tnode-`.
2. Reads `./bridge.toml` from the current directory (no global fallback).
   A missing or malformed file is treated as an empty config (warning only).
3. Resolves the final binary path (applying `[aliases]`), prepends `[args]`
   entries, and injects `[env]` variables.
4. Finds `libthook.so` via `find_thook_lib()` (checks `$THOOK_LIB_PATH`,
   `<exe>/../lib/libthook.so`, same-dir, debug/release Cargo targets).
5. Sets `LD_PRELOAD`, `THOOK_TMPDIR`, `THOOK_DEBUG_LEVEL`, `THOOK_LOG_FILE`,
   and `npm_config_android_ndk_path` on the child `Command`.
6. Calls `Command::status()` and propagates the exit code.

On `EACCES` from `Command::status()`, `main` applies the same shebang and
ELF Copier logic as `libthook` (at the process level, before `exec`).

### `--tnode-env-export`

Prints shell `export` statements for `LD_PRELOAD`, `THOOK_TMPDIR`, and the
`[env]` block. Output is clean (no ANSI codes, no log prefixes) so it is
safe to pipe through `eval`.

### `--tnode-doctor`

Runs environment diagnostics (without relying on `which` being installed):
1. Verifies `libthook.so` existence and accessibility.
2. Validates TMPDIR existence and writability.
3. Checks available free disk space in TMPDIR via `statvfs(2)`. Warns if < 50MB.
4. Locates active `bridge.toml` across parent directories.
5. Performs direct `$PATH` lookups for essential build tools (`node`, `npm`, `clang`, `python`, `bun`, `git`).
6. Detects whether CWD is on internal storage or an SDCARD FUSE mount.
7. Reads Android API level (`ro.build.version.sdk`) from `/system/build.prop` and warns about Android 12+ (API >= 31) Phantom Process Killer.
8. Checks SELinux enforcing status (`/sys/fs/selinux/enforce`).

Exits with 0 on success or 1 on critical errors.

### `--tnode-stats`

Displays a terminal table summarizing cache usage in `$TMPDIR` grouped by file prefix:
- `thook_exec_*`: Cached ELF binary copies.
- `thook_dl_*`: Cached shared library (`.node`) files.
- `tnode_exec_*`: Cached process-level ELF copies.

Reports count, total size formatted in B/KB/MB, and age of the oldest file.

### `--tnode-completions <bash|zsh|fish>`

Outputs shell completion scripts for `bash`, `zsh`, or `fish`. Compatible with `eval "$(tnode --tnode-completions bash)"` or redirecting into completion directories.

### `--tnode-clean` / `--tnode-clean-max <MB>`

- `--tnode-clean`: Deletes `thook_exec_*`, `thook_dl_*`, and `tnode_exec_*` files in `$TMPDIR` older than 24 hours.
- `--tnode-clean-max <MB>`: Deletes any cache files exceeding the specified threshold in megabytes, regardless of age.

### `--tnode-hook-log <hooks>`

Forces trace-level logging for specific comma-separated hook names (e.g. `dlopen,execve`) by injecting `THOOK_LOG_HOOKS`. Allows debugging specific interception points without enabling global trace logs.

### `--tnode-update`

Downloads and installs the latest release from `https://github.com/Irithell/tnode/releases`.

Flow:
1. Calls the GitHub Releases API via `curl` and extracts `tag_name` from the JSON response.
2. Detects the running architecture (`env::consts::ARCH`) and platform (checks for `/data/data/com.termux`).
3. Downloads the matching binary asset (`tnode-<arch>-<platform>`) and library asset (`libthook-<arch>-<platform>.so`) to a temporary staging directory (`$TMPDIR/tnode_update_download/`).
4. Replaces the current binary and library atomically via `fs::copy` + `fs::rename` to avoid a partially-written state.
5. Sets `chmod 0755` on the installed binary via `PermissionsExt`.
6. Displays the changelog panel for the new version immediately after completion via `print_changelog(Some(&latest_ver))`.

Do not verify checksums inside the Rust binary; checksum verification is the responsibility of `install.sh`.

### `--tnode-changelog [version|latest|all]`

Renders the embedded `CHANGELOG.md` (included at compile time via `include_str!("../../../CHANGELOG.md")`) as a terminal panel with colored status indicators:
- `\x1b[32m●\x1b[0m` — Added (green)
- `\x1b[34m●\x1b[0m` — Changed (blue)
- `\x1b[31m●\x1b[0m` — Removed (red)

Accepted arguments:
- No argument or `latest`: renders only the first (most recent) version block.
- `all`: renders the full history.
- A specific version string (`0.1.1` or `v0.1.1`): renders that version block only.

The CHANGELOG parser is line-based. It identifies version blocks by lines starting with `## [`, and section headings by lines starting with `### `. The `print_changelog` function must never make network calls; it reads only from the compile-time embedded constant.

### Background update check

On every normal command execution, `check_updates_background()` is called before launching the child process:
1. Reads `~/.cache/tnode/last_check` (a Unix timestamp in seconds).
2. If more than 86400 seconds have elapsed, spawns a detached `std::thread` that calls `fetch_latest_release_tag()` via `curl`.
3. The thread writes the retrieved tag to `~/.cache/tnode/latest_version` and the current timestamp to `~/.cache/tnode/last_check`.
4. After the child exits, `print_pending_update_notice()` reads `~/.cache/tnode/latest_version`. If the cached tag differs from `VERSION`, a one-line notice is printed to stderr.

The background thread must never block the main execution path. It is fire-and-forget.

### `bridge.toml` discovery (`find_bridge_toml`)

`find_bridge_toml()` walks parent directories starting from CWD looking for `bridge.toml`. The upward traversal stops as soon as a `bridge.toml` is found, or when hitting:
- `$HOME` (`/data/data/com.termux/files/home`)
- `/storage` or `/sdcard` mount points
- Filesystem root (`/`)

### Path validation (`is_suitable_tmp_dir`)

`is_suitable_tmp_dir(path: &str) -> bool` is called before accepting any custom temporary directory (from `--tnode-tmp`, `bridge.toml [core] tmp`, or `$THOOK_TMPDIR`).

Returns `false` if:
- The path contains `/storage/` or `/sdcard/` (FUSE noexec mount).
- `fs::create_dir_all` fails.
- A write probe (`fs::write` + `fs::remove_file` on `.tnode_writable_test`) fails.

On failure, `tnode` emits a `[tnode:WARN]` message and falls back to `$TMPDIR` (or the Termux internal tmp hardcoded path if that also fails). This fallback chain must never silently proceed with an invalid tmp directory.

The equivalent guard in `libthook` is `get_valid_tmp_dir()`, which rejects paths containing `/storage/` or `/sdcard/` at the pattern level (without a write probe, since `libthook` runs in an injected context where stdio and filesystem probes would be re-entrant risks).

### Configured `lib` path validation

When a custom `libthook.so` path is provided (via `--tnode-lib` or `bridge.toml [core] lib`), `tnode` checks that `path.exists() && path.is_file()` before using it. If the check fails, a `[tnode:WARN]` is emitted and the auto-discovery fallback (`find_thook_lib()`) is used instead. Discovery order:
1. `$THOOK_LIB_PATH`
2. `<exe>/../lib/libthook.so`
3. Same directory as the `tnode` binary
4. Cargo debug/release target paths (development only)

### NDK injection (`npm_config_android_ndk_path`)

For processes whose name contains `npm`, `npx`, `yarn`, `pnpm`, or
`node-gyp`, `tnode` sets `npm_config_android_ndk_path=dummy`. This satisfies
`node-gyp`'s check for a non-empty NDK path value. Using `ANDROID_NDK_PATH`
instead causes npm to emit an "Unknown env config" warning; the `npm_config_`
prefix avoids the warning while still being read by `node-gyp`'s JS
environment detection layer.

---

## Configuration reference (`bridge.toml`)

```toml
[core]
enabled = true          # Master switch. false disables aliases/args/env injection.
tmp = "..."             # ELF cache and FIFO redirect directory.
lib = "..."             # Path to libthook.so.
log = "thook-{timestamp}.log"  # Log file. {timestamp} and {pid} macros supported.
debug_level = "off"    # "off" | "error" | "warn" | "info" | "trace"
hook_log = "dlopen,execve"  # Force trace logging for specific hooks
root = true             # Anchors this directory as root for bridge.toml discovery

[aliases]
node = "/path/to/node"  # Short name -> full binary path

[args]
npm = ["--ignore-scripts"]  # Args prepended before user args for this command

[env]
PRISMA_CLIENT_ENGINE_TYPE = "binary"  # Injected into the child environment

[hooks]
# Disable specific syscall hooks per project (all true by default).
# Valid names: realpath, symlink, symlinkat, chmod, fchmodat, flock, fcntl,
#              syscall, execve, execvp, dlopen, mkfifo, mkfifoat, open, open64,
#              openat, openat64, stat, lstat, stat64, lstat64, link, linkat,
#              unlink, unlinkat, readlink, readlinkat
# realpath = false
# dlopen = false
```

`bridge.toml` is discovered by walking parent directories up to `$HOME` or `/sdcard`.
It is scoped per project tree.

---

## Debugging

Set `THOOK_DEBUG_LEVEL=trace` and `THOOK_LOG_FILE=/tmp/thook.log` to get a
full trace of every intercepted call. The `{timestamp}` macro in the log path
prevents runs from overwriting each other.

```sh
tnode --tnode-debug trace --tnode-log /tmp/thook-{timestamp}.log <command>
```

Or from the shell without `tnode`:

```sh
export LD_PRELOAD="$PREFIX/lib/libthook.so"
export THOOK_DEBUG_LEVEL=trace
export THOOK_LOG_FILE=/tmp/thook.log
node script.js
```

---

## LD_PRELOAD boundary and runtime categorization

`libthook.so` intercepts only calls that pass through `libc.so`. The
following table summarizes expected behavior per runtime:

| Runtime | Mechanism | Interceptable? |
|---|---|---|
| C / C++ | Links to `libc.so` | Yes — 100% |
| Node.js (V8/libuv) | Links to `libc.so` | Yes — 100% |
| Python | Links to `libc.so` | Yes — 100% |
| Ruby | Links to `libc.so` | Yes — 100% |
| Bun (JS runtime) | JavaScriptCore links to `libc.so` | Yes — `bun run` works |
| Go (1.26+) | `SVC #0` inline assembly | No (file locks fixed upstream) |
| Zig / Bun installer | `SVC #0` inline assembly | No (`bun install --backend=copyfile`) |
| JVM/Dalvik | Mixed (depends on JNI linking) | Partial |

### Runtime categorization & upstream fixes

- **Synthesis of `LD_PRELOAD` boundaries**:
  - Runtimes dependent on `libc.so` (Node.js, Python, Ruby, C/C++) -> Interceptable and resolved via `LD_PRELOAD`.
  - Runtimes issuing raw syscalls via inline assembly (`SVC #0`) -> `LD_PRELOAD` is mathematically blind. Solutions require direct patches in the runtime's upstream repository.

- **Go — `FcntlFlock` on SDCARD**:
  - *Symptom:* `go: RLock /sdcard/projeto/go.mod: function not implemented` (`ENOSYS` from FUSE).
  - *Cause:* Go issues `SVC #0` in `syscall_linux_arm64.s` bypassing `libc.so`.
  - *Upstream Resolution:* Reported in Issue **#30831** of `termux/termux-packages`. The fix was applied in PR **#30833** redirecting the file locking backend from `flock` to `fcntl`.
  - *Resolved Version:* Package `golang` revision **`3:1.26.5-1`** or higher.

- **Bun — `linkat` on SDCARD**:
  - *Symptom:* `error: Failed to link typescript: EACCES` when running `bun install` on SDCARD.
  - *Cause:* Bun's package manager is written in Zig and issues raw `linkat` syscalls via assembly (`SVC #0`). SDCARD FUSE returns `EACCES`/`EPERM`. Bun previously only handled `EXDEV` to trigger physical copy fallback.
  - *Upstream Resolution:* Reported in Issue **#36852** of `oven-sh/bun`. Fix merged in PR **#36853** by Jarred Sumner on 2026-08-04, extending copy fallback to `EACCES` and `EPERM` errors (covering hoisted and isolated linkers).
  - *Current Workaround:* `bun install --backend=copyfile` until an official release after 2026-08-04 is published.

---

## Known issues and edge cases

1. **`getcwd` must not be intercepted.** An attempt to hook `getcwd` to hide
   `/storage/emulated` from Go regressions caused `node-gyp` (Python) to
   compute wrong relative paths across mount points, breaking `bcrypt` and
   other native addons. The `getcwd` hook was removed. The esbuild CWD issue
   is handled specifically inside the `execve` hook instead.

2. **`realpath` memory ownership.** When the caller passes `resolved_path = NULL`,
   libc allocates the result with `malloc`. If the opaque strategy replaces
   this result, the original must be freed with `libc::free` before returning
   the new `malloc_cstring` buffer. Never use `CString::into_raw` for values
   that will be freed by C code.

3. **Export map (`export.map`).** Only the symbols listed as `global` are
   exported. Internal helpers (`malloc_cstring`, `copy_elf_to_tmp`, etc.) are
   `local: *`. The `dlopen` export refers to the assembly trampoline, not
   `thook_dlopen_rust`. If new public hooks are added they must be added to
   `export.map`.

4. **`IS_NODE` guard on `realpath`.** Do not remove this guard. Without it,
   tools like `clang` fail to find CRT objects (`crtbegin_so.o`) because the
   opaque strategy hides real Termux paths.

5. **esbuild CWD hack.** The `set_current_dir` call inside the `execve` hook
   for esbuild changes the CWD of the calling process (the Node process that
   called `child_process.exec` or similar). This is intentional but means the
   Node process's CWD is permanently changed after esbuild runs. Applications
   that depend on CWD after running esbuild should restore it explicitly.

6. **`thook_dl_*` files are not executable.** `chmod` is not applied to
   dlopen-cached `.node` files because they are loaded by `dlopen`, not
   `execve`. Only `thook_exec_*` files receive `chmod 0700`.

7. **Prisma engines require `engineType = "binary"`.** The official Prisma
   engines are compiled for glibc and crash on Bionic. Use custom Bionic
   engines from `https://github.com/Irithell/prisma-termux/releases`. The
   schema must set `engineType = "binary"` so Prisma uses `execve` (triggering
   the ELF Copier bypass) instead of `dlopen` (which would try the Library
   Engine). For `npx prisma studio`, set `PRISMA_CLIENT_ENGINE_TYPE=binary`
   explicitly because it bypasses the schema.

8. **Android Phantom Process Killer.** On Android 12+, the system may SIGKILL
   child processes spawned by npm (worker threads for package extraction) if
   they exceed the background process limit. This is an Android OS constraint
   and not a bug in this project.

---

## Agent behavior rules

1. **Target environment**: Termux on Android only. Do not add macOS
   (`DYLD_INSERT_LIBRARIES`) or Windows compatibility code.

2. **Annotation log**: Before marking any major step complete, you can write an entry in  `anotation.md`.

3. **No fake translations**: the `realpath` opaque logic must use real paths
   from the environment. Do not hardcode or mock path translations.

4. **Memory safety in `realpath`**: always use `libc::malloc` / `libc::free`
   for buffers returned to C callers, never `CString::into_raw`.

5. **Compiler protection**: the `IS_NODE` guard on `realpath` must remain.
   Removing it breaks cross-compilation with `clang`.

6. **Do not add a `getcwd` hook**: it breaks Python-based build tools.

7. **Export map**: any new exported symbol must be added to `export.map`.

8. **`.cargo/config.toml`**: must remain in `.gitignore`. Never commit it.

9. **`npm_config_android_ndk_path`**: use this exact variable name, not
   `ANDROID_NDK_PATH`, to avoid npm internal warnings.

10. **Debug level variable**: use `THOOK_DEBUG_LEVEL` with string values
    (`error`/`warn`/`info`/`trace`). The legacy `THOOK_DEBUG=1` is still
    accepted for backwards compatibility but not preferred.

11. **`print_changelog` must not make network calls.** The function renders only the compile-time embedded `CHANGELOG` constant. Never add HTTP requests to it.

12. **Background update thread is fire-and-forget.** `check_updates_background()` must not be awaited, joined, or allowed to delay the main execution path in any way.

13. **`is_suitable_tmp_dir` must be called before accepting any custom tmp path.** Never pass a user-supplied or environment-supplied tmp path directly to ELF Copier, FIFO, or dlopen cache operations without first validating writability and confirming the path is not on an SDCARD FUSE mount.

14. **`get_valid_tmp_dir` in `libthook` is a pattern-only guard.** It checks for `/storage/` or `/sdcard/` in the path string. Do not add write probes or filesystem calls inside it; `libthook` runs injected into arbitrary processes where blocking I/O in hook context is unsafe.

15. **Changelog is embedded, not loaded at runtime.** `CHANGELOG` is included via `include_str!("../../../CHANGELOG.md")` at compile time. If the changelog is modified after building, the binary must be recompiled for the change to be reflected.
