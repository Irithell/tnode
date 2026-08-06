<p align="center">
  <img src="https://pub-5cd6924d31ff4d3aae9f8df4b3ae81f4.r2.dev/17a37b3760e65972e7536646a8828411" width="180" alt="TNode Logo" />
</p>

<h1 align="center">tnode / thook</h1>

<p align="center">
  <a href="https://github.com/Irithell/tnode/releases/latest"><img src="https://img.shields.io/github/v/release/Irithell/tnode?label=release&color=4a90e2" alt="Latest Release" /></a>
  <a href="https://github.com/Irithell/tnode"><img src="https://img.shields.io/badge/platform-Android%20%7C%20Termux-brightgreen" alt="Platform" /></a>
  <a href="https://github.com/Irithell/tnode/releases/latest"><img src="https://img.shields.io/badge/arch-aarch64%20%7C%20x86__64-orange" alt="Architecture" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Irithell/tnode" alt="License" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/built%20with-Rust-b7410e" alt="Rust" /></a>
</p>

<p align="center">
  <b>A POSIX compatibility shim and Node.js bridge for Android Termux.</b>
</p>

`libthook.so` is injected into any process via `LD_PRELOAD` and intercepts
libc calls (`realpath`, `execve`, `dlopen`, `flock`, `fcntl`, `symlink`,
`mkfifo`, `link`, and others) to transparently paper over Android's SDCARD
filesystem restrictions. `tnode` is the CLI wrapper that configures the
environment and launches any command with the library pre-loaded.

<p align="center">
  <img src="https://pub-5cd6924d31ff4d3aae9f8df4b3ae81f4.r2.dev/cda2e45e8a7ee513b36445b55321a7a5" width="100%" alt="TNode Banner" />
</p>

---

## Table of contents

<div align="center">

[![How it Works](https://img.shields.io/badge/How_it_Works-1e1e2e?style=for-the-badge&labelColor=89b4fa&color=181825)](#how-it-works)
[![Project Structure](https://img.shields.io/badge/Project_Structure-1e1e2e?style=for-the-badge&labelColor=b4befe&color=181825)](#project-structure)
[![Installation](https://img.shields.io/badge/Installation-1e1e2e?style=for-the-badge&labelColor=cba6f7&color=181825)](#installation)
[![Building](https://img.shields.io/badge/Building-1e1e2e?style=for-the-badge&labelColor=a6e3a1&color=181825)](#building-from-source)
[![Usage](https://img.shields.io/badge/Usage-1e1e2e?style=for-the-badge&labelColor=f9e2af&color=181825)](#usage)
[![bridge.toml](https://img.shields.io/badge/bridge.toml-1e1e2e?style=for-the-badge&labelColor=fab387&color=181825)](#bridgetoml)
[![Diagnostics](https://img.shields.io/badge/Diagnostics-1e1e2e?style=for-the-badge&labelColor=f38ba8&color=181825)](#diagnostics)
[![Environment](https://img.shields.io/badge/Environment-1e1e2e?style=for-the-badge&labelColor=74c7ec&color=181825)](#environment-variables)
[![Ecosystem](https://img.shields.io/badge/Ecosystem-1e1e2e?style=for-the-badge&labelColor=94e2d5&color=181825)](#ecosystem-compatibility)

</div>

---

## How it works

Android's SDCARD (`/storage/emulated/0`, `/sdcard`) is mounted with FUSE
using a vfat-compatible layer. This mount has several restrictions that break
standard POSIX tooling:

- **noexec**: the kernel refuses to `execve` any file on the SDCARD, even ELF
  binaries and shell scripts.
- **No symlinks**: `symlink(2)` fails with `EACCES`/`EPERM`.
- **No hard links**: `link(2)` / `linkat(2)` fail with `EACCES`.
- **No file locks**: `flock(2)` and `fcntl(2)` locking commands return
  `ENOSYS`/`EACCES`.
- **No special inodes**: `mkfifo(3)` fails with `EACCES`.
- **Bionic linker namespace**: `dlopen` blocks loading `.node` native addons
  from SDCARD paths.

`libthook.so` intercepts each of these operations and applies a bypass:

| Failing operation | Bypass |
|---|---|
| `execve`/`execvp` on noexec | Shebang rewrite (shell scripts) or ELF Copier (native binaries) |
| `symlink`/`symlinkat` | Wrapper shell script (`.bin`) or physical copy |
| `chmod`/`fchmodat` | Silently return 0 |
| `flock`/`fcntl`/`syscall` locking | Return 0 when fd is on SDCARD |
| `mkfifo`/`mkfifoat` | Redirect creation to `$TMPDIR` |
| `open`/`openat`/`stat` on a missing pipe | Redirect to `$TMPDIR` counterpart |
| `dlopen` from SDCARD | Copy to `$TMPDIR`, open the copy |
| `link`/`linkat` | Physical recursive copy fallback |
| `realpath` (Node only) | Opaque strategy: return original path, hide Termux prefix |

### ELF Copier bypass

When `execve` returns `EACCES` on an ELF binary on the SDCARD, the hook
copies it to `$TMPDIR/thook_exec_<hash>_<name>`. The hash is derived from the
absolute path, mtime, and size so that the cache is automatically invalidated
if the binary changes. The copy is `chmod 0700` and executed from Termux
internal storage where `noexec` does not apply.

### Shebang bypass (termux-exec emulation)

Scripts with `#!/bin/sh`, `#!/bin/bash`, or `#!/usr/bin/env` in `node_modules/.bin`
are intercepted. The interpreter path is rewritten to `$PREFIX/bin/sh` (or
equivalent) so that execution goes through the Termux shell instead of
Android's `/system/bin/sh`, which drops `LD_PRELOAD` due to linker namespace
restrictions.

### FIFO (named pipe) redirection

`mkfifo` failures on SDCARD are redirected to a deterministically-named path
under `$TMPDIR`. All subsequent `open`, `stat`, and `unlink` calls for the
original SDCARD path are transparently forwarded to the `$TMPDIR` counterpart.

### dlopen namespace bypass

Android 7+ uses Bionic linker namespaces that prevent loading `.node` native
addons from SDCARD paths. The `dlopen` hook copies the file to `$TMPDIR` using
the same cache-invalidation scheme as the ELF Copier and opens the copy.

### Realpath opaque strategy

Node.js resolves symlink chains via `realpath`. If a project on the SDCARD
contains symlinks pointing into the Termux prefix, Node would see the real
Termux path and break `require` resolution. The hook intercepts `realpath`
and, when the calling process is identified as Node, returns the original
(pre-symlink) path instead. Compilers and other tools always see real paths.

### Known limits (LD_PRELOAD boundary)

`LD_PRELOAD` only intercepts calls routed through `libc.so`. Runtimes like
**Go** and **Zig** execute syscalls via inline Assembly (`SVC #0`) and are
completely invisible to `libthook.so`. For those runtimes, upstream fixes are
required:

- **Go `flock`**: Fixed in `termux/termux-packages` golang revision
  `3:1.26.5-1` (issue #30831, PR #30833).
- **Bun `linkat`**: Fixed in `oven-sh/bun` PR #36853. Until a release ships,
  use `bun install --backend=copyfile`.

## Project structure

```
.
├── Cargo.toml             Workspace root
├── .cargo/
│   └── config.toml        NDK linker paths (local only, not committed)
├── crates/
│   ├── thook/             Library crate -> libthook.so (LD_PRELOAD hook)
│   │   ├── Cargo.toml
│   │   ├── build.rs       Passes --version-script to the linker
│   │   ├── export.map     Controls which symbols are exported
│   │   └── src/lib.rs
│   └── tnode/             Binary crate -> tnode (CLI wrapper)
│       ├── Cargo.toml
│       └── src/main.rs
├── dist/                  Pre-built release binaries
│   ├── tnode-aarch64-android
│   ├── tnode-x86_64-android
│   ├── libthook-aarch64-android.so
│   ├── libthook-x86_64-android.so
│   └── checksums.sha256
└── install.sh             One-line installer
```

## Installation

### One-line installer (recommended)

```sh
curl -fsSL https://github.com/Irithell/tnode/releases/latest/download/install.sh | bash
```

Options:

```sh
bash install.sh --version v0.1.0   # pin a specific version
bash install.sh --prefix /custom   # custom install prefix
bash install.sh --no-verify        # skip SHA-256 verification
```

The installer places `tnode` in `$PREFIX/bin/` and `libthook.so` in
`$PREFIX/lib/`.

### Manual installation

Download the assets for your architecture from the
[releases page](https://github.com/Irithell/tnode/releases), verify checksums,
and install:

```sh
# aarch64 (most Android phones)
install -m755 tnode-aarch64-android "$PREFIX/bin/tnode"
install -m644 libthook-aarch64-android.so "$PREFIX/lib/libthook.so"
```

## Building from source

### Prerequisites

- Rust toolchain (stable)
- Android NDK (tested with NDK r26 / API 24)
- AArch64 and/or x86_64 Android targets:

```sh
rustup target add aarch64-linux-android
rustup target add x86_64-linux-android   # optional
```

### Configuring the NDK linker

`.cargo/config.toml` contains absolute paths to the NDK `clang` linkers and
**must not be committed** to version control (it is in `.gitignore`). Each
developer must create their own copy:

```toml
# .cargo/config.toml  (create this file manually, do not commit it)

[target.aarch64-linux-android]
linker = "/path/to/ndk/<version>/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"

[target.x86_64-linux-android]
linker = "/path/to/ndk/<version>/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android24-clang"
```

Replace `/path/to/ndk/<version>` with the actual path to your NDK installation.
On a typical Linux setup with the Android SDK this is usually
`$HOME/Android/Sdk/ndk/<version>`.

The NDK can be installed via Android Studio (SDK Manager -> NDK) or by
downloading it directly from the
[NDK releases page](https://github.com/android/ndk/releases).

### Build commands

```sh
# Release build for aarch64
cargo build --target aarch64-linux-android --release

# Release build for x86_64
cargo build --target x86_64-linux-android --release
```

Artifacts are placed in `target/<triple>/release/`:
- `libthook.so` — the LD_PRELOAD hook library
- `tnode` — the CLI wrapper binary

## Usage

### Quick start

```sh
# Run npm with the hook active
tnode npm install canvas

# Run npx with debug logging
tnode --tnode-debug info npx prisma db push

# Pipe debug output to a file with per-run timestamps
tnode --tnode-debug trace --tnode-log ./thook-{timestamp}.log npm run build
```

### CLI flags (tnode-prefixed to avoid conflicts with wrapped commands)

| Flag | Description |
|---|---|
| `--tnode-help` | Print help and exit |
| `--tnode-version` | Print version and exit |
| `--tnode-update` | Download and install the latest release atomically |
| `--tnode-changelog [version\|latest\|all]` | Display the changelog panel (default: latest version) |
| `--tnode-stats` | Show cache usage statistics grouped by file type |
| `--tnode-completions <shell>` | Generate shell completion script (`bash`, `zsh`, `fish`) |
| `--tnode-lib <path>` | Use a custom `libthook.so` |
| `--tnode-tmp <path>` | Override the ELF cache directory |
| `--tnode-log <path>` | Write hook logs to a file (`{timestamp}` and `{pid}` macros supported) |
| `--tnode-debug <level>` | Log level: `off`, `error`, `warn`, `info`, `trace` |
| `--tnode-hook-log <hooks>` | Force trace-level logs for specific hooks (e.g. `dlopen,execve`) |
| `--tnode-clean` | Delete cached ELF/library files older than 24 h |
| `--tnode-clean-max <MB>` | Delete cache files exceeding a size threshold (in MB) |
| `--tnode-init` | Generate a default `bridge.toml` in the current directory |
| `--tnode-bridge-off` | Ignore `bridge.toml` for this invocation |
| `--tnode-bridge-on` | Force-enable `bridge.toml` even if `core.enabled = false` |
| `--tnode-env` | Print the active configuration and environment |
| `--tnode-env-export` | Print `export` statements for use with `eval` |
| `--tnode-doctor` | Run diagnostics (checks PATH, TMPDIR space, API level, SELinux) |

### bridge.toml

`tnode` searches for `bridge.toml` starting from the current working directory,
walking up parent directories until `$HOME` or `/sdcard`. Create a template
with `tnode --tnode-init`.

```toml
[core]
enabled = true
tmp = "/data/data/com.termux/files/usr/tmp"
lib = "/data/data/com.termux/files/usr/lib/libthook.so"
# log = "thook-{timestamp}.log"
debug_level = "off"
# hook_log = "dlopen,execve"    # force trace-level logs for specific hooks
root = true                     # anchor as root directory for bridge.toml discovery

[aliases]
# Map short names to full binary paths
node = "/data/data/com.termux/files/usr/bin/node"

[args]
# Arguments prepended to specific commands
# npm = ["--ignore-scripts"]

[env]
# Environment variables injected into the child process
# PRISMA_QUERY_ENGINE_LIBRARY = "/path/to/libquery_engine.so.node"

[hooks]
# Disable specific hooks per project (all enabled by default)
# realpath = false
# dlopen = false
```

### Using the environment export (eval mode)

To run commands directly without going through `tnode`, export its environment:

```sh
eval "$(tnode --tnode-env-export)"
node my-script.js
```

### Diagnostics

```sh
tnode --tnode-doctor
```

Checks for `libthook.so`, writable `$TMPDIR`, free disk space via `statvfs(2)`,
`bridge.toml` discovery, essential tools in `$PATH`, Android API level, and SELinux status.

## Environment variables

| Variable | Description |
|---|---|
| `LD_PRELOAD` | Set by `tnode` to the `libthook.so` path |
| `THOOK_DEBUG_LEVEL` | Log level (`error`, `warn`, `info`, `trace`) |
| `THOOK_LOG_FILE` | Path for log output (supports `{timestamp}`, `{pid}`) |
| `THOOK_LOG_HOOKS` | Comma-separated hook names to log at trace level |
| `THOOK_DISABLE_HOOKS` | Comma-separated hook names to disable at runtime |
| `THOOK_TMPDIR` | Directory for ELF cache and FIFO redirects |
| `THOOK_LIB_PATH` | Explicit override for `libthook.so` discovery |
| `PREFIX` | Termux prefix (default: `/data/data/com.termux/files/usr`) |

## Scope of this tool

`tnode` and `libthook.so` solve one specific class of problem: libc-level
filesystem and linker restrictions imposed by Android's SDCARD FUSE mount.
They do not install, manage, or provide any build toolchains, system
libraries, native compilers, or external runtimes.

If a native addon requires a C compiler, a system library, or a
pre-compiled binary targeted at the Android AArch64 ABI, that dependency
must be separately installed and made available in the environment before
`tnode npm install` is invoked. The sections below document the exact
requirements for commonly used packages.

---

## Ecosystem compatibility

The following sections describe the system-level requirements for packages
that are frequently used in Node.js projects on Termux and that need
additional setup beyond `npm install`.

### APT flags used in this section

Several install commands in this section include flags that are non-default
for standard Debian/Ubuntu APT usage. On some Android devices, the APT
sandbox mechanism (which drops privileges before invoking the package
manager backend) does not operate correctly because the system user namespace
or `setuid` support is restricted at the kernel or firmware level. The flag
`-o APT::Sandbox::User=root` instructs APT to skip the privilege drop and
run the backend directly under the current user, which is root within the
Termux environment. The flag `--allow-unauthenticated` suppresses signature
verification errors that can occur when pulling packages from community
repositories (`tur-repo`, `x11-repo`) that may not have their signing keys
fully registered in the local keyring at install time. These flags should
be used only within Termux; they are not appropriate for general Debian
system administration.

---

### canvas

The `canvas` package requires GTK and Cairo development headers plus several
image format libraries to compile its native C++ addon at install time.

**System dependencies:**

```sh
pkg install x11-repo tur-repo -y -o APT::Sandbox::User=root
pkg update -y -o APT::Sandbox::User=root
pkg install \
  build-essential clang make python pkg-config \
  pango libcairo libpng libjpeg-turbo giflib librsvg \
  xorgproto libx11 libxrender \
  -y -o APT::Sandbox::User=root --allow-unauthenticated
```

To install only the packages that are not already present, query the package
manager first and pass only the missing ones to `pkg install`. The following
shell snippet demonstrates the pattern:

```sh
PKGS="build-essential clang make python pkg-config pango libcairo libpng libjpeg-turbo giflib librsvg xorgproto libx11 libxrender"

MISSING=$(for p in $PKGS; do
  dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed" || echo "$p"
done)

if [ -n "$MISSING" ]; then
  pkg install $MISSING -y -o APT::Sandbox::User=root --allow-unauthenticated
fi
```

**Install canvas:**

```sh
tnode npm install canvas
```

---

### better-sqlite3

`better-sqlite3` compiles a native addon that links against the SQLite
amalgamation. The system SQLite headers and build tools must be present.

**System dependencies:**

```sh
pkg update -y -o APT::Sandbox::User=root
pkg install \
  build-essential clang make python pkg-config sqlite \
  -y -o APT::Sandbox::User=root --allow-unauthenticated
```

**Install better-sqlite3:**

```sh
tnode npm install better-sqlite3
```

---

### puppeteer

Puppeteer requires a Chromium binary targeted at the Android AArch64 ABI.
The upstream Chromium distributed through npm (`chromium` bundled by
puppeteer's postinstall script) is compiled for Linux x86_64 and cannot
execute on Android. Install the Termux-provided Chromium package instead
and configure puppeteer to use it.

**System dependencies:**

```sh
pkg upgrade -y && pkg install x11-repo tur-repo -y && pkg update -y
pkg install chromium -y
```

**Configuring puppeteer to use the system Chromium:**

Puppeteer must be told to skip its own Chromium download and to use the
system binary. At a minimum, `executablePath` must point to the installed
binary and the flags `--no-sandbox`, `--disable-setuid-sandbox`, and
`--disable-dev-shm-usage` must be passed because the Android kernel
configuration does not provide the namespace isolation that Chromium's
sandbox requires.

The following is an example of how to override the launch method in a
runtime configuration. Adapt the pattern to your application's structure
without modifying the puppeteer package itself:

```js
const puppeteer = require("puppeteer");

const browser = await puppeteer.launch({
  executablePath: "/data/data/com.termux/files/usr/bin/chromium-browser",
  args: [
    "--no-sandbox",
    "--disable-setuid-sandbox",
    "--disable-dev-shm-usage",
  ],
});
```

**Install puppeteer (without bundled Chromium):**

```sh
PUPPETEER_SKIP_DOWNLOAD=true tnode npm install puppeteer
```

---

### Prisma

The official Prisma query engine binaries are compiled for glibc-based Linux
and cannot be executed on Android Bionic. Pre-compiled engine binaries
targeting AArch64 Android are available:

<p align="left">
  <a href="https://github.com/Irithell/prisma-termux/releases">
    <img src="https://img.shields.io/badge/Get_Bionic_Prisma_Engines-2D3748?style=for-the-badge&logo=prisma&logoColor=5A67D8&labelColor=1A202C" alt="Get Bionic Prisma Engines" />
  </a>
</p>

If you require a version that is not listed there, open an issue in that
repository.

**Configuring Prisma to use the local engine binaries:**

Prisma must be instructed to use the binary engine type and to resolve each
engine binary from the directory where you placed the downloaded files. If
these variables are not set, Prisma defaults to the library engine type and
attempts to load a glibc-compiled shared object, which will fail on Bionic.

Set the following environment variables before importing the Prisma client,
or place them in the entry point of your application before any module
resolution occurs:

```js
const path = require("path");

const enginesDir = "/path/to/your/prisma/engines";

process.env.PRISMA_CLIENT_ENGINE_TYPE = "binary";
process.env.PRISMA_QUERY_ENGINE_BINARY = path.join(enginesDir, "query-engine");
process.env.PRISMA_SCHEMA_ENGINE_BINARY = path.join(enginesDir, "schema-engine");
process.env.PRISMA_FMT_BINARY = path.join(enginesDir, "prisma-fmt");
```

For commands that run outside your application entry point, such as
`npx prisma studio`, the engine type must be set explicitly in the shell
environment because the application bootstrap code does not run in that
context:

```sh
PRISMA_CLIENT_ENGINE_TYPE=binary tnode npx prisma studio
```

Alternatively, add it to the `[env]` block of your `bridge.toml` so that
`tnode` injects it automatically for every command:

```toml
[env]
PRISMA_CLIENT_ENGINE_TYPE = "binary"
```

