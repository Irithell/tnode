use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};
use ctor::ctor;
#[cfg(target_arch = "aarch64")]
use std::arch::global_asm;

// The dlopen interception on AArch64 requires a small trampoline written in inline assembly.
// The Rust compiler cannot generate the calling convention required to seamlessly fall through
// to the original symbol pointer after our Rust handler decides not to intercept a call.
// The trampoline stores the real dlopen pointer in a .data symbol (ORIG_DLOPEN_PTR), saves
// the original arguments on the stack, calls thook_dlopen_rust, and — if the Rust handler
// sets the call_orig flag — tail-calls the real dlopen via the stored pointer.
#[cfg(target_arch = "aarch64")]
global_asm!(
    ".data",
    ".align 3",
    "ORIG_DLOPEN_PTR:",
    ".xword 0",
    ".text",
    ".global set_orig_dlopen_ptr",
    ".type set_orig_dlopen_ptr, %function",
    "set_orig_dlopen_ptr:",
    "adrp x1, ORIG_DLOPEN_PTR",
    "str x0, [x1, #:lo12:ORIG_DLOPEN_PTR]",
    "ret",
    ".global get_orig_dlopen_ptr",
    ".type get_orig_dlopen_ptr, %function",
    "get_orig_dlopen_ptr:",
    "adrp x0, ORIG_DLOPEN_PTR",
    "ldr x0, [x0, #:lo12:ORIG_DLOPEN_PTR]",
    "ret",
    ".global dlopen",
    ".type dlopen, %function",
    "dlopen:",
    "stp x29, x30, [sp, #-48]!",
    "mov x29, sp",
    "stp x0, x1, [sp, #16]",
    "add x2, sp, #32",
    "strb wzr, [x2]",
    "bl thook_dlopen_rust",
    "ldrb w3, [sp, #32]",
    "cmp w3, #0",
    "b.ne 1f",
    "ldp x29, x30, [sp], #48",
    "ret",
    "1:",
    "ldp x0, x1, [sp, #16]",
    "ldp x29, x30, [sp], #48",
    "adrp x2, ORIG_DLOPEN_PTR",
    "ldr x2, [x2, #:lo12:ORIG_DLOPEN_PTR]",
    "br x2"
);

#[cfg(target_arch = "aarch64")]
extern "C" {
    fn set_orig_dlopen_ptr(ptr: *const libc::c_void);
    fn get_orig_dlopen_ptr() -> *const libc::c_void;
}

#[cfg(not(target_arch = "aarch64"))]
static ORIG_DLOPEN_PTR: std::sync::atomic::AtomicPtr<libc::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(not(target_arch = "aarch64"))]
unsafe fn set_orig_dlopen_ptr(ptr: *const libc::c_void) {
    ORIG_DLOPEN_PTR.store(ptr as *mut _, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(target_arch = "aarch64"))]
unsafe fn get_orig_dlopen_ptr() -> *const libc::c_void {
    ORIG_DLOPEN_PTR.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(not(target_arch = "aarch64"))]
#[no_mangle]
pub unsafe extern "C" fn dlopen(filename: *const c_char, flags: libc::c_int) -> *mut libc::c_void {
    let mut call_orig: libc::c_int = 0;
    let res = thook_dlopen_rust(filename, flags, &mut call_orig);
    if call_orig == 0 {
        return res;
    }
    let orig_ptr = get_orig_dlopen_ptr();
    if !orig_ptr.is_null() {
        let orig_fn: DlopenFn = std::mem::transmute(orig_ptr);
        return orig_fn(filename, flags);
    }
    std::ptr::null_mut()
}

// Function pointer types for each intercepted libc symbol.
// Each type mirrors the exact C signature of the corresponding libc function.
type RealpathFn = extern "C" fn(*const c_char, *mut c_char) -> *mut c_char;
type ReadlinkFn = extern "C" fn(*const c_char, *mut c_char, libc::size_t) -> libc::ssize_t;
type ReadlinkatFn = extern "C" fn(libc::c_int, *const c_char, *mut c_char, libc::size_t) -> libc::ssize_t;
type SymlinkFn = extern "C" fn(*const c_char, *const c_char) -> libc::c_int;
type SymlinkatFn = extern "C" fn(*const c_char, libc::c_int, *const c_char) -> libc::c_int;
type ChmodFn = extern "C" fn(*const c_char, libc::mode_t) -> libc::c_int;
type FchmodatFn = extern "C" fn(libc::c_int, *const c_char, libc::mode_t, libc::c_int) -> libc::c_int;
type ExecveFn = extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> libc::c_int;
type ExecvpFn = extern "C" fn(*const c_char, *const *const c_char) -> libc::c_int;
type DlopenFn = extern "C" fn(*const c_char, libc::c_int) -> *mut libc::c_void;
type FlockFn = extern "C" fn(libc::c_int, libc::c_int) -> libc::c_int;
type FcntlFn = extern "C" fn(libc::c_int, libc::c_int, *mut libc::c_void) -> libc::c_int;
type SyscallFn = extern "C" fn(libc::c_long, libc::c_long, libc::c_long, libc::c_long, libc::c_long, libc::c_long, libc::c_long) -> libc::c_long;
type MkfifoFn = extern "C" fn(*const c_char, libc::mode_t) -> libc::c_int;
type MkfifoatFn = extern "C" fn(libc::c_int, *const c_char, libc::mode_t) -> libc::c_int;
type LinkFn = extern "C" fn(*const c_char, *const c_char) -> libc::c_int;
type LinkatFn = extern "C" fn(libc::c_int, *const c_char, libc::c_int, *const c_char, libc::c_int) -> libc::c_int;
type OpenFn = extern "C" fn(*const c_char, libc::c_int, libc::mode_t) -> libc::c_int;
type OpenatFn = extern "C" fn(libc::c_int, *const c_char, libc::c_int, libc::mode_t) -> libc::c_int;
type StatFn = extern "C" fn(*const c_char, *mut libc::c_void) -> libc::c_int;
type LstatFn = extern "C" fn(*const c_char, *mut libc::c_void) -> libc::c_int;
type Stat64Fn = extern "C" fn(*const c_char, *mut libc::c_void) -> libc::c_int;
type Lstat64Fn = extern "C" fn(*const c_char, *mut libc::c_void) -> libc::c_int;
type UnlinkFn = extern "C" fn(*const c_char) -> libc::c_int;
type UnlinkatFn = extern "C" fn(libc::c_int, *const c_char, libc::c_int) -> libc::c_int;

// Lazily initialized slots for the real libc function pointers, resolved once at
// library load time via dlsym(RTLD_NEXT). OnceLock guarantees thread-safe
// single initialization without requiring a mutex on the hot path.
static ORIGINAL_REALPATH: OnceLock<RealpathFn> = OnceLock::new();
static ORIGINAL_READLINK: OnceLock<ReadlinkFn> = OnceLock::new();
static ORIGINAL_READLINKAT: OnceLock<ReadlinkatFn> = OnceLock::new();
static ORIGINAL_SYMLINK: OnceLock<SymlinkFn> = OnceLock::new();
static ORIGINAL_SYMLINKAT: OnceLock<SymlinkatFn> = OnceLock::new();
static ORIGINAL_CHMOD: OnceLock<ChmodFn> = OnceLock::new();
static ORIGINAL_FCHMODAT: OnceLock<FchmodatFn> = OnceLock::new();
static ORIGINAL_EXECVE: OnceLock<ExecveFn> = OnceLock::new();
static ORIGINAL_EXECVP: OnceLock<ExecvpFn> = OnceLock::new();
static ORIGINAL_DLOPEN: OnceLock<DlopenFn> = OnceLock::new();
static ORIGINAL_FLOCK: OnceLock<FlockFn> = OnceLock::new();
static ORIGINAL_FCNTL: OnceLock<FcntlFn> = OnceLock::new();
static ORIGINAL_SYSCALL: OnceLock<SyscallFn> = OnceLock::new();
static ORIGINAL_MKFIFO: OnceLock<MkfifoFn> = OnceLock::new();
static ORIGINAL_MKFIFOAT: OnceLock<MkfifoatFn> = OnceLock::new();
static ORIGINAL_LINK: OnceLock<LinkFn> = OnceLock::new();
static ORIGINAL_LINKAT: OnceLock<LinkatFn> = OnceLock::new();
static ORIGINAL_OPEN: OnceLock<OpenFn> = OnceLock::new();
static ORIGINAL_OPENAT: OnceLock<OpenatFn> = OnceLock::new();
static ORIGINAL_OPEN64: OnceLock<OpenFn> = OnceLock::new();
static ORIGINAL_OPENAT64: OnceLock<OpenatFn> = OnceLock::new();
static ORIGINAL_STAT: OnceLock<StatFn> = OnceLock::new();
static ORIGINAL_LSTAT: OnceLock<LstatFn> = OnceLock::new();
static ORIGINAL_STAT64: OnceLock<Stat64Fn> = OnceLock::new();
static ORIGINAL_LSTAT64: OnceLock<Lstat64Fn> = OnceLock::new();
static ORIGINAL_UNLINK: OnceLock<UnlinkFn> = OnceLock::new();
static ORIGINAL_UNLINKAT: OnceLock<UnlinkatFn> = OnceLock::new();

// IS_NODE is set once during init and determines whether the realpath opaque
// strategy should be applied. It is false for compilers and other non-Node tools
// that must see the real filesystem paths.
static IS_NODE: OnceLock<bool> = OnceLock::new();

// Cached Termux prefix string derived from the PREFIX environment variable.
static PREFIX: OnceLock<String> = OnceLock::new();

// Log level stored as an integer: 0=off, 1=error, 2=warn, 3=info, 4=trace.
// Written once during init, read on every hook invocation via Relaxed ordering.
static LOG_LEVEL: AtomicI32 = AtomicI32::new(0); // 0=off, 1=error, 2=warn, 3=info, 4=trace
// File descriptor for the log output. -1 means stderr.
static LOG_FD: AtomicI32 = AtomicI32::new(-1);

// Comma-separated list of hook names that are disabled at runtime via
// THOOK_DISABLE_HOOKS. Set by tnode from the bridge.toml [hooks] section.
// Example: "realpath,dlopen"
static DISABLED_HOOKS: OnceLock<String> = OnceLock::new();

// Comma-separated list of hook names that always emit trace-level logs,
// regardless of the global LOG_LEVEL. Set by tnode via THOOK_LOG_HOOKS.
// Example: "dlopen,execve"
static LOG_HOOKS: OnceLock<String> = OnceLock::new();

/// Writes `msg` followed by a newline to the configured log destination.
///
/// Uses `libc::write` directly to avoid any allocator or stdio re-entrancy
/// issues that could arise from calling Rust I/O inside a libc hook. The write
/// is a single atomic syscall so partial interleaving from concurrent threads
/// is bounded to one message boundary.
fn debug_log_internal(msg: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) > 0 {
        let fd = LOG_FD.load(Ordering::Relaxed);
        let mut out_msg = String::with_capacity(msg.len() + 1);
        out_msg.push_str(msg);
        out_msg.push('\n');
        
        unsafe {
            if fd >= 0 {
                libc::write(fd, out_msg.as_ptr() as *const libc::c_void, out_msg.len());
            } else {
                libc::write(libc::STDERR_FILENO, out_msg.as_ptr() as *const libc::c_void, out_msg.len());
            }
        }
    }
}

macro_rules! log_error {
    ($($arg:tt)*) => {
        if LOG_LEVEL.load(Ordering::Relaxed) >= 1 {
            debug_log_internal(&format!("\x1b[31m[thook:ERROR]\x1b[0m {}", format_args!($($arg)*)));
        }
    }
}

macro_rules! log_warn {
    ($($arg:tt)*) => {
        if LOG_LEVEL.load(Ordering::Relaxed) >= 2 {
            debug_log_internal(&format!("\x1b[33m[thook:WARN]\x1b[0m {}", format_args!($($arg)*)));
        }
    }
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        if LOG_LEVEL.load(Ordering::Relaxed) >= 3 {
            debug_log_internal(&format!("\x1b[36m[thook:INFO]\x1b[0m {}", format_args!($($arg)*)));
        }
    }
}

macro_rules! log_trace {
    ($($arg:tt)*) => {
        if LOG_LEVEL.load(Ordering::Relaxed) >= 4 {
            debug_log_internal(&format!("\x1b[35m[thook:TRACE]\x1b[0m {}", format_args!($($arg)*)));
        }
    }
}

/// Emits a trace-level log unconditionally if `hook_name` is in THOOK_LOG_HOOKS,
/// regardless of the global LOG_LEVEL. Falls through to the normal log_trace!
/// check otherwise. This allows per-hook selective logging without enabling full
/// trace mode.
macro_rules! log_hook_trace {
    ($hook:expr, $($arg:tt)*) => {
        if LOG_LEVEL.load(Ordering::Relaxed) >= 4 || is_hook_logged($hook) {
            debug_log_internal(&format!("\x1b[35m[thook:TRACE]\x1b[0m {}", format_args!($($arg)*)));
        }
    }
}

/// Resolves a libc symbol by name using `dlsym(RTLD_NEXT)`.
///
/// `RTLD_NEXT` is represented as `-1isize` because the `libc` crate does not
/// expose the `RTLD_NEXT` sentinel directly on Android Bionic targets.
///
/// Returns `Err(())` if the symbol cannot be resolved (library not loaded or
/// name typo). Callers should treat a missing symbol as a pass-through failure.
unsafe fn get_original_fn(name: &str) -> Result<*const libc::c_void, ()> {
    let c_name = CString::new(name).map_err(|_| ())?;
    let sym = libc::dlsym(-1isize as *mut libc::c_void, c_name.as_ptr());
    if sym.is_null() { Err(()) } else { Ok(sym) }
}

/// Library constructor executed immediately after `libthook.so` is mapped into
/// the process address space via `LD_PRELOAD`, before `main` or any other
/// constructor in the host process.
///
/// Responsibilities:
/// - Resolves and caches all original libc function pointers.
/// - Configures the log level from `THOOK_DEBUG_LEVEL` (or the legacy `THOOK_DEBUG=1`).
/// - Opens the log file if `THOOK_LOG_FILE` is set.
/// - Detects whether the host process is a Node.js instance and sets `IS_NODE`.
#[ctor(unsafe)]
fn init() {
    unsafe {
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"realpath\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_REALPATH.set(std::mem::transmute(sym)).ok(); }
        
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"readlink\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_READLINK.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"readlinkat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_READLINKAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"symlink\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_SYMLINK.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"symlinkat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_SYMLINKAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"chmod\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_CHMOD.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"fchmodat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_FCHMODAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"flock\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_FLOCK.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"fcntl\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_FCNTL.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"syscall\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_SYSCALL.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"mkfifo\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_MKFIFO.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"mkfifoat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_MKFIFOAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"link\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_LINK.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"linkat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_LINKAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"open\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_OPEN.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"openat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_OPENAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"open64\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_OPEN64.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"openat64\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_OPENAT64.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"stat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_STAT.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"lstat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_LSTAT.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"stat64\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_STAT64.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"lstat64\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_LSTAT64.set(std::mem::transmute(sym)).ok(); }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"unlink\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_UNLINK.set(std::mem::transmute(sym)).ok(); }
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"unlinkat\0".as_ptr() as *const _);
        if !sym.is_null() { ORIGINAL_UNLINKAT.set(std::mem::transmute(sym)).ok(); }

        if let Ok(ptr) = get_original_fn("execve") {
            ORIGINAL_EXECVE.set(std::mem::transmute(ptr)).ok();
        }
        if let Ok(ptr) = get_original_fn("execvp") {
            ORIGINAL_EXECVP.set(std::mem::transmute(ptr)).ok();
        }

        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"dlopen\0".as_ptr() as *const _);
        if !sym.is_null() {
            ORIGINAL_DLOPEN.set(std::mem::transmute(sym)).ok();
            // Store in the assembly-accessible global so the trampoline can tail-call it.
            set_orig_dlopen_ptr(sym);
        }
    }
        if let Ok(lvl) = std::env::var("THOOK_DEBUG_LEVEL") {
            let level = match lvl.as_str() {
                "error" => 1,
                "warn" => 2,
                "info" => 3,
                "trace" => 4,
                _ => 0,
            };
            LOG_LEVEL.store(level, Ordering::SeqCst);
        } else if let Ok(val) = std::env::var("THOOK_DEBUG") {
            // Legacy single-variable debug switch kept for backwards compatibility.
            if val == "1" {
                LOG_LEVEL.store(4, Ordering::SeqCst);
            }
        }

        if let Ok(disabled) = std::env::var("THOOK_DISABLE_HOOKS") {
            if !disabled.is_empty() {
                DISABLED_HOOKS.set(disabled).ok();
            }
        }

        if let Ok(log_hooks) = std::env::var("THOOK_LOG_HOOKS") {
            if !log_hooks.is_empty() {
                LOG_HOOKS.set(log_hooks).ok();
            }
        }
        
        if let Ok(log_file) = std::env::var("THOOK_LOG_FILE") {
            let c_path = CString::new(log_file).unwrap_or_default();
            unsafe {
                let fd = libc::open(c_path.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND, 0o666);
                if fd >= 0 {
                    LOG_FD.store(fd, Ordering::SeqCst);
                }
            }
        }
        
        log_info!("libthook.so initialized! Debug level: {}", LOG_LEVEL.load(Ordering::SeqCst));

    // Detect whether the host process is a Node.js instance so that the
    // realpath opaque strategy is not applied to compilers or other tools.
    if let Ok(exe) = std::env::current_exe() {
        let name = exe.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if name.contains("node") || name == "tnode" {
            IS_NODE.set(true).ok();
            log_trace!("[thook:init] INFO: libthook.so loaded into NodeJS process: {}", exe.display());
        } else {
            log_trace!("[thook:init] INFO: libthook.so loaded into Non-Node process: {}", exe.display());
        }
    }
}

/// Returns `true` if `hook_name` is present in the `THOOK_DISABLE_HOOKS` list.
///
/// Performs a zero-allocation scan over the comma-separated string stored in
/// the `DISABLED_HOOKS` static. Called at the entry of every exported hook
/// function to allow per-hook disabling via bridge.toml `[hooks]` section.
fn is_hook_disabled(hook_name: &str) -> bool {
    if let Some(list) = DISABLED_HOOKS.get() {
        for name in list.split(',') {
            if name.trim() == hook_name {
                return true;
            }
        }
    }
    false
}

/// Returns `true` if `hook_name` is present in the `THOOK_LOG_HOOKS` list,
/// meaning trace-level logs should be emitted for this hook regardless of
/// the global LOG_LEVEL setting.
fn is_hook_logged(hook_name: &str) -> bool {
    if let Some(list) = LOG_HOOKS.get() {
        for name in list.split(',') {
            if name.trim() == hook_name {
                return true;
            }
        }
    }
    false
}

/// Allocates a new C string on the libc heap using `libc::malloc`.
///
/// The returned pointer is compatible with `free()` called by libc consumers
/// such as the caller of `realpath` when `resolved_path` is `NULL`. Using
/// `CString::into_raw` would allocate via the Rust allocator, which may differ
/// from libc's allocator on Bionic and cause heap corruption on `free()`.
unsafe fn malloc_cstring(s: &str) -> *mut c_char {
    let c_string = CString::new(s).unwrap();
    let bytes = c_string.as_bytes_with_nul();
    let ptr = libc::malloc(bytes.len()) as *mut c_char;
    if !ptr.is_null() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, ptr, bytes.len());
    }
    ptr
}

/// Returns the absolute form of `path`.
///
/// If `path` is already absolute it is returned unchanged. Otherwise it is
/// joined to the current working directory. Falls back to returning the
/// original string if `current_dir()` fails.
fn absolute_path(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
            Err(_) => path.to_string(),
        }
    }
}

/// Recursively copies `src` to `dst`.
///
/// If `src` is a directory the entire tree is replicated under `dst`.
/// Used as a fallback when hard links or symlinks cannot be created on the
/// SDCARD FUSE filesystem.
fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
    } else {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

/// Returns true if `path` resides on an Android SDCARD / shared external storage mount.
/// Checks standard path prefix boundaries and resolves symlinks to avoid false positives.
fn is_sdcard_path(path: &str) -> bool {
    if path.starts_with("/storage/")
        || path.starts_with("/sdcard/")
        || path == "/sdcard"
        || path.starts_with("/mnt/sdcard/")
        || path.starts_with("/mnt/media_rw/")
    {
        return true;
    }
    if let Ok(canon) = std::fs::canonicalize(path) {
        let s = canon.to_string_lossy();
        if s.starts_with("/storage/")
            || s.starts_with("/sdcard/")
            || s == "/sdcard"
            || s.starts_with("/mnt/sdcard/")
            || s.starts_with("/mnt/media_rw/")
        {
            return true;
        }
    }
    false
}

/// Returns a valid, executable temporary directory for ELF caching and FIFO redirection.
/// If `THOOK_TMPDIR` or `TMPDIR` is located on an SDCARD / FUSE mount,
/// falls back to standard internal Termux storage (`/data/data/com.termux/files/usr/tmp`).
fn get_valid_tmp_dir() -> String {
    let tmp = std::env::var("THOOK_TMPDIR")
        .unwrap_or_else(|_| std::env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string()));
    if is_sdcard_path(&tmp) {
        "/data/data/com.termux/files/usr/tmp".to_string()
    } else {
        tmp
    }
}

/// Returns the path in `$TMPDIR` where a FIFO originally requested at
/// `original_path_str` should be physically created.
///
/// Returns `None` if `original_path_str` is not under `/storage/` or
/// `/sdcard/`, meaning the original path can be used directly.
///
/// The tmp path is derived from a hash of the absolute original path so that
/// the mapping is deterministic across calls and process restarts.
fn get_fifo_tmp_path(original_path_str: &str) -> Option<String> {
    if !original_path_str.starts_with("/storage/") && !original_path_str.starts_with("/sdcard/") {
        return None;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    let absolute = absolute_path(original_path_str);
    absolute.hash(&mut hasher);
    let hash_val = hasher.finish();
    
    let tmp_dir = get_valid_tmp_dir();
    
    let path = Path::new(original_path_str);
    let filename_only = path.file_name().unwrap_or_default().to_string_lossy();
    Some(format!("{}/thook_fifo_{}_{}", tmp_dir, hash_val, filename_only))
}

/// Implements the fake-symlink bypass for paths on the SDCARD.
///
/// For paths inside a `.bin` directory a POSIX shell wrapper script is written
/// instead of a real symlink. The wrapper reconstructs `basedir` at runtime so
/// that `__dirname` remains correct inside the target script. Returns `true`
/// always to signal that the symlink call should be reported as succeeded.
///
/// For non-binary paths (`.node` addons, `.js` files, etc.) a physical copy of
/// the target is made. If the target does not exist an empty file is created as
/// a placeholder to prevent the caller from crashing on a missing path.
fn handle_fake_symlink(target_str: &str, linkpath_str: &str) -> bool {
    let is_bin = linkpath_str.contains("/.bin/") || linkpath_str.contains("/bin/");
    
    if is_bin {
        let script_content = format!(
            "#!/bin/sh\nbasedir=$(dirname \"$0\")\nexec \"$basedir/{}\" \"$@\"",
            target_str
        );
        match std::fs::write(linkpath_str, script_content) {
            Ok(_) => {
                log_trace!("[thook:symlink_wrapper] SUCCESS: Fake symlink wrapper created for binary.");
                return true;
            },
            Err(e) => {
                log_trace!("[thook:symlink_wrapper] ERROR: Failed to create wrapper: {}", e);
                return true;
            }
        }
    } else {
        // Physical copy fallback for non-binaries (like .node or .js files)
        let link_path = Path::new(linkpath_str);
        let parent = link_path.parent().unwrap_or(Path::new(""));
        let target_path = parent.join(target_str);
        
        if target_path.exists() {
            if let Err(e) = copy_recursive(&target_path, link_path) {
                log_trace!("[thook:symlink_wrapper] ERROR: Failed to copy {:?} to {:?}: {}", target_path, link_path, e);
            } else {
                log_trace!("[thook:symlink_wrapper] SUCCESS: Physically copied {:?} to {:?}", target_path, link_path);
            }
        } else {
            log_trace!("[thook:symlink_wrapper] WARN: Target {:?} does not exist. Creating empty file to satisfy symlink.", target_path);
            let _ = std::fs::write(linkpath_str, "");
        }
        return true;
    }
}

/// Reads the shebang line from `filepath` and returns the interpreter arguments.
///
/// Reads at most 256 bytes. Returns `None` if the file cannot be opened, has
/// fewer than 2 bytes, or does not start with `#!`.
///
/// Interpreter paths are rewritten to their Termux equivalents:
/// - `/bin/sh` -> `$PREFIX/bin/sh`
/// - `/bin/bash` -> `$PREFIX/bin/bash`
/// - `/usr/bin/env` -> `$PREFIX/bin/env`
///
/// This rewrite is the termux-exec shebang bypass: it ensures that child
/// processes spawned from scripts in `node_modules/.bin` use the Termux shell
/// instead of Android's `/system/bin/sh`, which strips `LD_PRELOAD`.
fn get_shebang(filepath: &str) -> Option<Vec<String>> {
    use std::io::Read;
    let mut file = match std::fs::File::open(filepath) {
        Ok(f) => f,
        Err(_) => return None,
    };

    let mut buffer = [0u8; 256];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);
    if bytes_read < 2 {
        return None;
    }

    if buffer[0] == b'#' && buffer[1] == b'!' {
        let shebang_line = buffer.iter()
            .take(bytes_read)
            .take_while(|&&c| c != b'\n')
            .copied()
            .collect::<Vec<u8>>();
            
        if let Ok(line) = String::from_utf8(shebang_line) {
            let mut parts: Vec<String> = line[2..].split_whitespace().map(|s| s.to_string()).collect();
            if !parts.is_empty() {
                let prefix = std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
                if parts[0] == "/bin/sh" || parts[0] == "/bin/bash" {
                    let termux_sh = format!("{}/bin/{}", prefix, if parts[0] == "/bin/sh" { "sh" } else { "bash" });
                    if Path::new(&termux_sh).exists() {
                        parts[0] = termux_sh;
                    }
                } else if parts[0] == "/usr/bin/env" {
                    let direct = if parts.len() > 1 {
                        let candidate = format!("{}/bin/{}", prefix, parts[1]);
                        if Path::new(&candidate).exists() {
                            Some(candidate)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(direct_binary) = direct {
                        parts.remove(0);
                        parts[0] = direct_binary;
                    } else {
                        let termux_env = format!("{}/bin/env", prefix);
                        if Path::new(&termux_env).exists() {
                            parts[0] = termux_env;
                        } else {
                            parts[0] = "/usr/bin/env".to_string();
                        }
                    }
                }
                return Some(parts);
            }
        }
    }
    None
}

/// Returns `true` if the first four bytes of `filepath` are the ELF magic
/// number `\x7fELF`.
///
/// Used by the `execve`/`execvp` hooks to distinguish native ELF binaries
/// (which require the ELF Copier bypass) from shell scripts (which are handled
/// by the shebang bypass).
fn is_elf(filepath: &str) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(filepath) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buffer = [0u8; 4];
    if file.read_exact(&mut buffer).is_ok() {
        return buffer == [0x7f, b'E', b'L', b'F'];
    }
    false
}

/// Copies an ELF binary from the SDCARD to `$TMPDIR` and returns the tmp path.
///
/// The destination filename embeds a hash of the absolute source path, the
/// file's mtime, and its size. This acts as a cache-invalidation key: if the
/// binary changes on disk a new tmp copy is created automatically.
///
/// The copy is made executable (`0o700`). Returns `None` if the copy fails.
fn copy_elf_to_tmp(filepath: &str) -> Option<String> {
    let path = Path::new(filepath);
    
    let metadata = std::fs::metadata(path).ok()?;
    let absolute_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    
    let tmp_dir = get_valid_tmp_dir();
    
    std::fs::create_dir_all(&tmp_dir).unwrap_or_default();
    
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    
    // Cache Invalidation Mechanism: Hash absolute path + nanosecond mtime + size
    absolute_path.to_string_lossy().hash(&mut hasher);
    if let Ok(modified) = metadata.modified() {
        if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
            duration.as_secs().hash(&mut hasher);
            duration.subsec_nanos().hash(&mut hasher);
        }
    }
    metadata.len().hash(&mut hasher);
    let hash_val = hasher.finish();
    
    let filename_only = path.file_name().unwrap_or_default().to_string_lossy();
    let new_path_str = format!("{}/thook_exec_{}_{}", tmp_dir, hash_val, filename_only);
    let new_path = Path::new(&new_path_str);
    
    if !new_path.exists() {
        log_trace!("[thook:elf_cache] INFO: ELF binary '{}' not cached. Copying to '{}' (Hash: {})", filepath, new_path_str, hash_val);
        if std::fs::copy(filepath, &new_path_str).is_ok() {
            unsafe {
                let c_str = CString::new(new_path_str.clone()).unwrap();
                libc::chmod(c_str.as_ptr(), 0o700);
            }
        } else {
            log_trace!("[thook:elf_cache] ERROR: Failed to copy ELF binary to tmp dir.");
            return None;
        }
    } else {
        log_trace!("[thook:elf_cache] INFO: Using cached ELF binary at '{}' (Hash matched)", new_path_str);
    }
    Some(new_path_str)
}

/// Hook for `realpath(3)`.
///
/// Delegates to the original libc `realpath`. When the calling process is
/// identified as Node.js (`IS_NODE`) and the "opaque" strategy is active, it
/// additionally applies path translation: if the resolved canonical path falls
/// inside the Termux prefix but the originally requested path does not, the
/// hook returns the original (non-resolved) absolute path instead.
///
/// This prevents Node.js from seeing the real Termux prefix when the project
/// lives on the SDCARD, while keeping the real paths visible to compilers
/// (clang, make, python) that must resolve system library paths correctly.
///
/// Memory management: when `resolved_path` is `NULL` the caller expects a
/// `malloc`-allocated result. If the opaque translation triggers in that case,
/// the hook frees the original libc-allocated result and returns a new
/// `malloc`-allocated string so the caller can safely call `free()`.
#[no_mangle]
pub unsafe extern "C" fn realpath(path: *const c_char, resolved_path: *mut c_char) -> *mut c_char {
    let orig_realpath = match ORIGINAL_REALPATH.get() {
        Some(f) => f,
        None => return std::ptr::null_mut(),
    };

    if is_hook_disabled("realpath") {
        return orig_realpath(path, resolved_path);
    }

    if path.is_null() {
        return orig_realpath(path, resolved_path);
    }

    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return orig_realpath(path, resolved_path),
    };

    let result_ptr = orig_realpath(path, resolved_path);
    
    if result_ptr.is_null() {
        return result_ptr;
    }
    
    let result_str = match CStr::from_ptr(result_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return result_ptr,
    };
    let resolved_path_buf = PathBuf::from(result_str);

    let is_node = *IS_NODE.get().unwrap_or(&false);
    if is_node {
        let termux_prefix = PREFIX.get_or_init(|| {
            std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string())
        });
        let termux_prefix_normalized = termux_prefix.trim_end_matches('/');
        let resolved_str = resolved_path_buf.to_string_lossy().to_string();
        
        if resolved_str.starts_with(termux_prefix_normalized) {
            let path_abs = absolute_path(path_str);
            if !path_abs.starts_with(termux_prefix_normalized) && result_str.starts_with(termux_prefix_normalized) {
                log_hook_trace!("realpath", "[thook:realpath] ACTION: Applied opaque strategy. Redirected resolution from '{}' to '{}' [ptr: {:p}]", result_str, path_abs, path);
                
                if !resolved_path.is_null() {
                    let path_abs_c = CString::new(path_abs).unwrap();
                    let bytes = path_abs_c.as_bytes_with_nul();
                    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, resolved_path, bytes.len());
                    return resolved_path;
                } else {
                    let new_ptr = malloc_cstring(&path_abs);
                    if !new_ptr.is_null() {
                        libc::free(result_ptr as *mut libc::c_void);
                        return new_ptr;
                    }
                }
            }
        }
    }

    result_ptr
}

/// Hook for `readlink(2)`.
///
/// Currently a transparent pass-through to the original libc function.
/// The hook exists as an intercept point so that future strategy changes
/// (e.g., rewriting symlink targets on SDCARD) can be added here without
/// changing the ABI exported from the library.
#[no_mangle]
pub unsafe extern "C" fn readlink(pathname: *const c_char, buf: *mut c_char, bufsiz: libc::size_t) -> libc::ssize_t {
    let orig_readlink = match ORIGINAL_READLINK.get() {
        Some(f) => f,
        None => return -1,
    };
    if is_hook_disabled("readlink") {
        return orig_readlink(pathname, buf, bufsiz);
    }
    orig_readlink(pathname, buf, bufsiz)
}

/// Hook for `readlinkat(2)`.
///
/// Transparent pass-through. Exists as a future intercept point for
/// directory-relative symlink reads.
#[no_mangle]
pub unsafe extern "C" fn readlinkat(dirfd: libc::c_int, pathname: *const c_char, buf: *mut c_char, bufsiz: libc::size_t) -> libc::ssize_t {
    let orig_readlinkat = match ORIGINAL_READLINKAT.get() {
        Some(f) => f,
        None => return -1,
    };
    if is_hook_disabled("readlinkat") {
        return orig_readlinkat(dirfd, pathname, buf, bufsiz);
    }
    orig_readlinkat(dirfd, pathname, buf, bufsiz)
}

/// Hook for `symlink(2)`.
///
/// Attempts the real symlink. If the kernel returns `EACCES` or `EPERM` on a
/// path under `/storage/` or `/sdcard/` (SDCARD FUSE filesystem), the hook
/// activates the fake-symlink bypass: for `.bin` entries a wrapper shell
/// script is written; for other files a physical copy is made. Returns 0 to
/// report success to the caller.
#[no_mangle]
pub unsafe extern "C" fn symlink(target: *const c_char, linkpath: *const c_char) -> libc::c_int {
    let orig_symlink = match ORIGINAL_SYMLINK.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("symlink") {
        return orig_symlink(target, linkpath);
    }

    let res = orig_symlink(target, linkpath);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let linkpath_str = if !linkpath.is_null() { CStr::from_ptr(linkpath).to_string_lossy().into_owned() } else { String::new() };
        let target_str = if !target.is_null() { CStr::from_ptr(target).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES || err == libc::EPERM {
            if is_sdcard_path(&linkpath_str) {
                log_trace!("[thook:symlink] WARN: EACCES/EPERM (errno: {}) detected on storage. Engaging fake symlink wrapper for '{}' -> '{}'", err, linkpath_str, target_str);
                if handle_fake_symlink(&target_str, &linkpath_str) {
                    return 0;
                }
            }
        } else {
            log_trace!("[thook:symlink] ERROR: Failed with errno {}", err);
        }
    }
    res
}

/// Hook for `symlinkat(2)`.
///
/// Same bypass logic as the `symlink` hook but for directory-relative symlink
/// creation. The fake-symlink wrapper is used when the fully qualified link
/// path is on the SDCARD.
#[no_mangle]
pub unsafe extern "C" fn symlinkat(target: *const c_char, newdirfd: libc::c_int, linkpath: *const c_char) -> libc::c_int {
    let orig_symlinkat = match ORIGINAL_SYMLINKAT.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("symlinkat") {
        return orig_symlinkat(target, newdirfd, linkpath);
    }

    let res = orig_symlinkat(target, newdirfd, linkpath);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let linkpath_str = if !linkpath.is_null() { CStr::from_ptr(linkpath).to_string_lossy().into_owned() } else { String::new() };
        let target_str = if !target.is_null() { CStr::from_ptr(target).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES || err == libc::EPERM {
            if is_sdcard_path(&linkpath_str) {
                log_trace!("[thook:symlinkat] WARN: EACCES/EPERM (errno: {}) detected on storage. Engaging fake symlink wrapper for '{}' -> '{}'", err, linkpath_str, target_str);
                handle_fake_symlink(&target_str, &linkpath_str);
                return 0; 
            }
        } else {
            log_trace!("[thook:symlinkat] ERROR: Failed with errno {}", err);
        }
    }
    res
}

/// Hook for `chmod(2)`.
///
/// The SDCARD FUSE filesystem does not support permission bits. When `chmod`
/// returns `EACCES` or `EPERM` on a SDCARD path the hook silently returns 0
/// so that package managers do not abort installation on a permission error
/// that has no practical effect on the filesystem.
#[no_mangle]
pub unsafe extern "C" fn chmod(pathname: *const c_char, mode: libc::mode_t) -> libc::c_int {
    let orig_chmod = match ORIGINAL_CHMOD.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("chmod") {
        return orig_chmod(pathname, mode);
    }

    let res = orig_chmod(pathname, mode);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES || err == libc::EPERM {
            if is_sdcard_path(&pathname_str) {
                log_trace!("[thook:chmod] WARN: EACCES/EPERM (errno: {}) detected on storage for '{}' (mode: {:o}). Forcing success (0).", err, pathname_str, mode);
                return 0;
            }
        } else {
            log_trace!("[thook:chmod] ERROR: Failed with errno {}", err);
        }
    }
    res
}

/// Hook for `fchmodat(2)`.
///
/// Same bypass as `chmod` for the directory-relative variant.
#[no_mangle]
pub unsafe extern "C" fn fchmodat(dirfd: libc::c_int, pathname: *const c_char, mode: libc::mode_t, flags: libc::c_int) -> libc::c_int {
    let orig_fchmodat = match ORIGINAL_FCHMODAT.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("fchmodat") {
        return orig_fchmodat(dirfd, pathname, mode, flags);
    }

    let res = orig_fchmodat(dirfd, pathname, mode, flags);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES || err == libc::EPERM {
            if is_sdcard_path(&pathname_str) {
                log_trace!("[thook:fchmodat] WARN: EACCES/EPERM (errno: {}) detected on storage for '{}' (mode: {:o}). Forcing success (0).", err, pathname_str, mode);
                return 0;
            }
        } else {
            log_trace!("[thook:fchmodat] ERROR: Failed with errno {}", err);
        }
    }
    res
}

/// Resolves the filesystem path currently open on file descriptor `fd` using
/// `/proc/self/fd/<fd>`. Returns `None` if the path cannot be read.
fn get_path_from_fd(fd: libc::c_int) -> Option<String> {
    let proc_path = format!("/proc/self/fd/{}", fd);
    std::fs::read_link(&proc_path).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Hook for `flock(2)`.
///
/// The SDCARD FUSE/f2fs filesystem returns `ENOSYS`, `EACCES`, or `EPERM`
/// for `flock` calls. Runtimes such as Python, Ruby, and some C libraries
/// treat a failed lock as a fatal error. When the hook detects a failure on
/// an fd that resolves to a path under `/storage/` or `/sdcard/`, it returns
/// 0 to simulate a successful lock acquisition. Hard locking semantics are
/// intentionally not enforced because the SDCARD filesystem has no atomic
/// locking capability anyway.
#[no_mangle]
pub unsafe extern "C" fn flock(fd: libc::c_int, operation: libc::c_int) -> libc::c_int {
    let orig_flock = match ORIGINAL_FLOCK.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("flock") {
        return orig_flock(fd, operation);
    }

    let res = orig_flock(fd, operation);
    
    if res == -1 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::ENOSYS || err == libc::EACCES || err == libc::EPERM || err == libc::EBADF {
            if let Some(path) = get_path_from_fd(fd) {
                if is_sdcard_path(&path) {
                    // SDCARD FUSE does not implement kernel flock(2).
                    // Emulate flock via POSIX fcntl(F_SETLK/F_SETLKW) which is supported by kernel VFS
                    // across processes, providing true mutual exclusion.
                    let is_nb = (operation & libc::LOCK_NB) != 0;
                    let op_type = operation & !libc::LOCK_NB;
                    let fcntl_cmd = if is_nb { libc::F_SETLK } else { libc::F_SETLKW };
                    let lock_type = match op_type {
                        libc::LOCK_SH => Some(libc::F_RDLCK),
                        libc::LOCK_EX => Some(libc::F_WRLCK),
                        libc::LOCK_UN => Some(libc::F_UNLCK),
                        _ => None,
                    };

                    if let Some(l_type) = lock_type {
                        let mut flk = libc::flock {
                            l_type: l_type as libc::c_short,
                            l_whence: libc::SEEK_SET as libc::c_short,
                            l_start: 0,
                            l_len: 0,
                            l_pid: 0,
                        };
                        let fcntl_res = if let Some(orig_fc) = ORIGINAL_FCNTL.get() {
                            orig_fc(fd, fcntl_cmd, &mut flk as *mut _ as *mut libc::c_void)
                        } else {
                            libc::fcntl(fd, fcntl_cmd, &mut flk as *mut _ as *mut libc::c_void)
                        };

                        if fcntl_res == 0 {
                            log_trace!("[thook:flock] ACTION: Emulated flock via fcntl on SDCARD for fd {}. Real mutual exclusion active.", fd);
                            return 0;
                        } else {
                            let fcntl_err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                            if fcntl_err == libc::EAGAIN || fcntl_err == libc::EWOULDBLOCK || fcntl_err == libc::EDEADLK {
                                log_trace!("[thook:flock] ACTION: Contested lock on fd {} (errno {}). Preserving error for mutual exclusion.", fd, fcntl_err);
                                return -1;
                            }
                        }
                    }

                    // Fallback to safe success simulation only if fcntl emulation is also not supported
                    log_trace!("[thook:flock] ACTION: Ignored ENOSYS/EACCES on SDCARD for fd {} ('{}'). Forcing success.", fd, path);
                    return 0;
                }
            }
        }
        log_trace!("[thook:flock] ERROR: Failed on fd {} with errno {}", fd, err);
    }
    res
}

/// Hook for `fcntl(2)`.
///
/// Same file-locking bypass as `flock` but for `fcntl`-based locks (e.g.,
/// `F_SETLK`, `F_SETLKW`). Intercepting this is necessary because some
/// runtimes use `fcntl` instead of `flock` for POSIX advisory locks.
#[no_mangle]
pub unsafe extern "C" fn fcntl(fd: libc::c_int, cmd: libc::c_int, arg: *mut libc::c_void) -> libc::c_int {
    let orig_fcntl = match ORIGINAL_FCNTL.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("fcntl") {
        return orig_fcntl(fd, cmd, arg);
    }

    let res = orig_fcntl(fd, cmd, arg);

    if res == -1 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::ENOSYS || err == libc::EACCES || err == libc::EPERM || err == libc::EBADF {
            if let Some(path) = get_path_from_fd(fd) {
                if is_sdcard_path(&path) {
                    log_trace!("[thook:fcntl] ACTION: Ignored ENOSYS/EACCES on SDCARD for fd {} ('{}') cmd {}. Forcing success.", fd, path, cmd);
                    return 0;
                }
            }
        }
        log_trace!("[thook:fcntl] ERROR: Failed on fd {} cmd {} with errno {}", fd, cmd, err);
    }
    res
}

/// Hook for `syscall(2)` variadic wrapper in libc.
///
/// Intercepts `SYS_fcntl` and `SYS_flock` (syscall number 73 on aarch64).
/// Some runtimes call these syscalls through the libc `syscall()` wrapper
/// rather than the dedicated `flock`/`fcntl` functions. The same SDCARD
/// bypass logic applies.
///
/// Note: runtimes like Go and Zig that emit raw `SVC #0` instructions bypass
/// this hook entirely. Only syscalls routed through libc are interceptable.
#[no_mangle]
pub unsafe extern "C" fn syscall(
    sysno: libc::c_long,
    a1: libc::c_long,
    a2: libc::c_long,
    a3: libc::c_long,
    a4: libc::c_long,
    a5: libc::c_long,
    a6: libc::c_long,
) -> libc::c_long {
    let orig_syscall = match ORIGINAL_SYSCALL.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("syscall") {
        return orig_syscall(sysno, a1, a2, a3, a4, a5, a6);
    }

    log_trace!("[thook:syscall] TRACE: Intercepted syscall sysno {}", sysno);
    let res = orig_syscall(sysno, a1, a2, a3, a4, a5, a6);
    if res == -1 {
        // SYS_fcntl and SYS_flock intercept for Go bypassing libc direct functions
        if sysno == libc::SYS_fcntl || sysno == libc::SYS_flock {
            let fd = a1 as libc::c_int;
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == libc::ENOSYS || err == libc::EACCES || err == libc::EPERM || err == libc::EBADF {
                if let Some(path) = get_path_from_fd(fd) {
                    if is_sdcard_path(&path) {
                        log_trace!("[thook:syscall] ACTION: Ignored ENOSYS/EACCES on SDCARD for sysno {} fd {} ('{}'). Forcing success.", sysno, fd, path);
                        return 0;
                    }
                }
            }
        }
    }
    res
}

/// Hook for `execve(2)`.
///
/// Attempts the original `execve`. On `EACCES` (the filesystem has the
/// `noexec` mount flag set, which is the case for the SDCARD FUSE mount) two
/// bypass strategies are tried in order:
///
/// 1. **Shebang bypass**: reads the first 256 bytes of the target file looking
///    for a `#!` shebang. If found, reconstructs the argument vector with the
///    interpreter at argv[0] and re-executes via `libc::execvp`. The shebang
///    paths are rewritten to Termux equivalents to avoid dropping `LD_PRELOAD`
///    via Android's system shell.
///
/// 2. **ELF Copier bypass**: checks for the ELF magic bytes. If the file is a
///    native ELF binary (e.g., esbuild, prisma engines) it is copied to
///    `$TMPDIR` on Termux internal storage where `noexec` does not apply.
///    For esbuild specifically, all relative path arguments (`--outfile`,
///    `--outdir`, bare entry-point paths) are rewritten to absolute paths
///    before `execve` so that the Go runtime's `os.Getwd()` does not fail when
///    the working directory is on the SDCARD (Go calls `SYS_getcwd` directly
///    and Android blocks reads on `/storage/emulated` without explicit
///    permission). The process CWD is also temporarily changed to
///    `/data/data/com.termux/files/usr/tmp` for the esbuild case.
#[no_mangle]
pub unsafe extern "C" fn execve(
    filename: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> libc::c_int {
    let orig_execve = match ORIGINAL_EXECVE.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("execve") {
        return orig_execve(filename, argv, envp);
    }

    // Call orig_execve first without any pre-allocations.
    // In multi-threaded programs post-fork, heap allocations before execve risk deadlock.
    // If orig_execve succeeds, it never returns.
    let res = orig_execve(filename, argv, envp);
    if res == -1 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let filename_str = if !filename.is_null() { CStr::from_ptr(filename).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES {
            if let Some(shebang_args) = get_shebang(&filename_str) {
                log_trace!("[thook:execve] INFO: EACCES on '{}', parsed shebang: {:?}", filename_str, shebang_args);
                
                let mut new_args = Vec::new();
                for arg in shebang_args.iter() {
                    new_args.push(CString::new(arg.clone()).unwrap());
                }
                new_args.push(CString::new(filename_str.clone()).unwrap());
                
                let mut i = 1;
                if !argv.is_null() {
                    while !(*argv.offset(i)).is_null() {
                        new_args.push(CStr::from_ptr(*argv.offset(i)).to_owned());
                        i += 1;
                    }
                }
                
                let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                new_argv.push(std::ptr::null());
                
                log_trace!("[thook:execve] ACTION: Rewriting execution to shebang target with preserved envp.");
                // Preserve envp! Never discard envp via bare execvp
                return orig_execve(new_argv[0], new_argv.as_ptr(), envp);
            } else if is_elf(&filename_str) {
                log_trace!("[thook:execve] WARN: EACCES on ELF binary '{}'. Engaging ELF Copier Bypass...", filename_str);
                if let Some(copied_path) = copy_elf_to_tmp(&filename_str) {
                    log_trace!("[thook:execve] SUCCESS: Execution redirected to cached ELF.");
                    
                    let copied_cstr = CString::new(copied_path.clone()).unwrap();
                    let mut new_args = Vec::new();
                    new_args.push(copied_cstr.clone());
                    
                    let mut i = 1;
                    if !argv.is_null() {
                        while !(*argv.offset(i)).is_null() {
                            let arg_cstr = CStr::from_ptr(*argv.offset(i));
                            let mut arg_str = arg_cstr.to_string_lossy().to_string();
                            
                            // ESBUILD ANDROID PATH FIX
                            if copied_path.contains("esbuild") {
                                if !arg_str.starts_with("-") && !arg_str.starts_with("/") {
                                    if let Ok(cwd) = std::env::current_dir() {
                                        arg_str = format!("{}/{}", cwd.display(), arg_str);
                                    }
                                } else if arg_str.starts_with("--outfile=") && !arg_str.contains("=/") {
                                    if let Ok(cwd) = std::env::current_dir() {
                                        arg_str = arg_str.replace("--outfile=", &format!("--outfile={}/", cwd.display()));
                                    }
                                } else if arg_str.starts_with("--outdir=") && !arg_str.contains("=/") {
                                    if let Ok(cwd) = std::env::current_dir() {
                                        arg_str = arg_str.replace("--outdir=", &format!("--outdir={}/", cwd.display()));
                                    }
                                }
                            }
                            
                            new_args.push(CString::new(arg_str).unwrap());
                            i += 1;
                        }
                    }
                    
                    let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                    new_argv.push(std::ptr::null());
                    
                    let orig_execve = match ORIGINAL_EXECVE.get() {
                        Some(f) => f,
                        None => return -1,
                    };
                    
                    // CHDIR HACK FOR ESBUILD
                    if copied_path.contains("esbuild") {
                        if let Ok(cwd) = std::env::current_dir() {
                            let cwd_str = cwd.to_string_lossy();
                            if cwd_str.contains("/storage/emulated") || cwd_str.contains("/sdcard") {
                                log_trace!("[thook:execve] ESBUILD HACK: changing cwd to /data/data/com.termux/files/usr/tmp");
                                let _ = std::env::set_current_dir("/data/data/com.termux/files/usr/tmp");
                            }
                        }
                    }
                    
                    return orig_execve(copied_cstr.as_ptr(), new_argv.as_ptr(), envp);
                }
            } else if is_sdcard_path(&filename_str) && Path::new(&filename_str).is_file() {
                let prefix = std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
                let sh_path = format!("{}/bin/sh", prefix);
                let interp = if Path::new(&sh_path).exists() { sh_path } else { "/bin/sh".to_string() };
                let interp_cstr = CString::new(interp).unwrap();

                let mut new_args = Vec::new();
                new_args.push(interp_cstr.clone());
                new_args.push(CString::new(filename_str.clone()).unwrap());

                let mut i = 1;
                if !argv.is_null() {
                    while !(*argv.offset(i)).is_null() {
                        new_args.push(CStr::from_ptr(*argv.offset(i)).to_owned());
                        i += 1;
                    }
                }

                let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                new_argv.push(std::ptr::null());

                let orig_execve = match ORIGINAL_EXECVE.get() {
                    Some(f) => f,
                    None => return -1,
                };

                log_trace!("[thook:execve] ACTION: SDCARD script without shebang '{}', invoking via sh.", filename_str);
                return orig_execve(new_argv[0], new_argv.as_ptr(), envp);
            }
        } else {
            log_trace!("[thook:execve] ERROR: Failed to execute '{}' with errno {}", filename_str, err);
        }
    }
    res
}

/// Hook for `execvp(3)`.
///
/// Same bypass logic as `execve` (shebang rewrite and ELF Copier) but for the
/// PATH-searching variant. The esbuild-specific path absolutization and CWD
/// fixup are not applied here because `execvp` is not used for the esbuild
/// binary path in practice.
#[no_mangle]
pub unsafe extern "C" fn execvp(
    filename: *const c_char,
    argv: *const *const c_char,
) -> libc::c_int {
    let orig_execvp = match ORIGINAL_EXECVP.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("execvp") {
        return orig_execvp(filename, argv);
    }

    let res = orig_execvp(filename, argv);
    if res == -1 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let filename_str = if !filename.is_null() { CStr::from_ptr(filename).to_string_lossy().into_owned() } else { String::new() };
        if err == libc::EACCES {
            if let Some(shebang_args) = get_shebang(&filename_str) {
                log_trace!("[thook:execvp] INFO: EACCES on '{}', parsed shebang: {:?}", filename_str, shebang_args);
                
                let mut new_args = Vec::new();
                for arg in shebang_args.iter() {
                    new_args.push(CString::new(arg.clone()).unwrap());
                }
                new_args.push(CString::new(filename_str.clone()).unwrap());
                
                let mut i = 1;
                if !argv.is_null() {
                    while !(*argv.offset(i)).is_null() {
                        new_args.push(CStr::from_ptr(*argv.offset(i)).to_owned());
                        i += 1;
                    }
                }
                
                let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                new_argv.push(std::ptr::null());
                
                log_trace!("[thook:execvp] ACTION: Rewriting execution to shebang target.");
                return libc::execvp(new_argv[0], new_argv.as_ptr());
            } else if is_elf(&filename_str) {
                log_trace!("[thook:execvp] WARN: EACCES on ELF binary '{}'. Engaging ELF Copier Bypass...", filename_str);
                if let Some(copied_path) = copy_elf_to_tmp(&filename_str) {
                    let copied_cstr = CString::new(copied_path).unwrap();
                    
                    let mut new_args = Vec::new();
                    new_args.push(copied_cstr.clone());
                    
                    let mut i = 1;
                    if !argv.is_null() {
                        while !(*argv.offset(i)).is_null() {
                            new_args.push(CStr::from_ptr(*argv.offset(i)).to_owned());
                            i += 1;
                        }
                    }
                    
                    let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                    new_argv.push(std::ptr::null());
                    
                    log_trace!("[thook:execvp] SUCCESS: Execution redirected to cached ELF.");
                    return libc::execvp(copied_cstr.as_ptr(), new_argv.as_ptr());
                }
            } else if is_sdcard_path(&filename_str) && Path::new(&filename_str).is_file() {
                let prefix = std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
                let sh_path = format!("{}/bin/sh", prefix);
                let interp = if Path::new(&sh_path).exists() { sh_path } else { "/bin/sh".to_string() };
                let interp_cstr = CString::new(interp).unwrap();

                let mut new_args = Vec::new();
                new_args.push(interp_cstr.clone());
                new_args.push(CString::new(filename_str.clone()).unwrap());

                let mut i = 1;
                if !argv.is_null() {
                    while !(*argv.offset(i)).is_null() {
                        new_args.push(CStr::from_ptr(*argv.offset(i)).to_owned());
                        i += 1;
                    }
                }

                let mut new_argv: Vec<*const c_char> = new_args.iter().map(|s| s.as_ptr()).collect();
                new_argv.push(std::ptr::null());

                log_trace!("[thook:execvp] ACTION: SDCARD script without shebang '{}', invoking via sh.", filename_str);
                return libc::execvp(new_argv[0], new_argv.as_ptr());
            }
        } else {
            log_trace!("[thook:execvp] ERROR: Failed to execute '{}' with errno {}", filename_str, err);
        }
    }
    res
}

/// Rust handler called from the AArch64 `dlopen` trampoline.
///
/// The trampoline saves the original `filename` and `flags` arguments and
/// passes a pointer to a `call_orig` byte. Setting `*call_orig = 1` instructs
/// the trampoline to tail-call the real `dlopen`; leaving it `0` means this
/// function returns the result directly.
///
/// When `filename` points to a path under `/storage/` or `/sdcard/`, Android's
/// Bionic linker namespace restrictions prevent loading native `.node` addons
/// directly. The hook copies the file to `$TMPDIR` (same cache-invalidation
/// scheme as `copy_elf_to_tmp`) and opens the copy instead.
///
/// Paths under `/system/`, `/apex/`, `/vendor/`, and well-known system
/// libraries are allowed to pass through to the original `dlopen` without
/// inspection.
#[no_mangle]
pub unsafe extern "C" fn thook_dlopen_rust(
    filename: *const c_char,
    flags: libc::c_int,
    call_orig: *mut libc::c_int,
) -> *mut libc::c_void {
    let mut orig_dlopen_ptr = get_orig_dlopen_ptr();
    if orig_dlopen_ptr.is_null() {
        let sym = libc::dlsym(-1isize as *mut libc::c_void, b"dlopen\0".as_ptr() as *const _);
        if sym.is_null() {
            *call_orig = 0;
            return std::ptr::null_mut();
        }
        set_orig_dlopen_ptr(sym);
        orig_dlopen_ptr = sym;
    }
    
    let orig_dlopen: DlopenFn = std::mem::transmute(orig_dlopen_ptr);

    if !filename.is_null() {
        if let Ok(name) = CStr::from_ptr(filename).to_str() {
            // Pass system/framework libraries straight through to the real dlopen.
            if name.starts_with("/system/")
                || name.starts_with("/apex/")
                || name.starts_with("/vendor/")
                || name == "libc.so"
                || name == "libz.so"
                || name == "libm.so"
                || name == "libdl.so"
            {
                *call_orig = 1;
                return std::ptr::null_mut();
            }
        }
    } else {
        // NULL filename is a valid dlopen call (returns handle to current process).
        *call_orig = 1;
        return std::ptr::null_mut();
    }
    
    let filename_str = CStr::from_ptr(filename).to_string_lossy().into_owned();

    if is_hook_disabled("dlopen") {
        *call_orig = 1;
        return std::ptr::null_mut();
    }

    log_trace!("[thook:dlopen] TRACE: Attempting to load shared library '{}' (flags: {})", filename_str, flags);
    
    if is_sdcard_path(&filename_str) {
        log_trace!("[thook:dlopen] WARN: Android Bionic linker namespace restriction detected on '{}'. Engaging cache bypass...", filename_str);
        
        let path = Path::new(&filename_str);
        if path.exists() {
            if let Ok(metadata) = std::fs::metadata(path) {
                let absolute_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                
                let tmp_dir = get_valid_tmp_dir();
                std::fs::create_dir_all(&tmp_dir).unwrap_or_default();
                
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                
                // Same cache-invalidation scheme as copy_elf_to_tmp.
                absolute_path.to_string_lossy().hash(&mut hasher);
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        duration.as_secs().hash(&mut hasher);
                        duration.subsec_nanos().hash(&mut hasher);
                    }
                }
                metadata.len().hash(&mut hasher);
                let hash_val = hasher.finish();
                
                let filename_only = path.file_name().unwrap_or_default().to_string_lossy();
                let new_path_str = format!("{}/thook_dl_{}_{}", tmp_dir, hash_val, filename_only);
                let new_path = Path::new(&new_path_str);
                
                if !new_path.exists() {
                    log_trace!("[thook:dlopen] INFO: Caching shared library to '{}'", new_path_str);
                    let _ = std::fs::copy(&filename_str, &new_path_str);
                }
                
                let new_c_str = CString::new(new_path_str.clone()).unwrap();
                let res = orig_dlopen(new_c_str.as_ptr(), flags);
                if res.is_null() {
                    let err = CStr::from_ptr(libc::dlerror()).to_string_lossy();
                    log_trace!("[thook:dlopen] ERROR: Failed to load cached shared library: {}", err);
                } else {
                    log_trace!("[thook:dlopen] SUCCESS: Library loaded from cache [ptr: {:p}]", res);
                }
                return res;
            }
        } else {
             log_trace!("[thook:dlopen] ERROR: Library '{}' does not exist.", filename_str);
        }
    }
    
    // Library is not on SDCARD; let the trampoline call the real dlopen.
    *call_orig = 1;
    std::ptr::null_mut()
}

/// Hook for `mkfifo(3)`.
///
/// The SDCARD FUSE filesystem does not support special inodes. When `mkfifo`
/// fails with `EACCES`, `EPERM`, or `ENOSYS` on an SDCARD path, the hook
/// redirects the creation to a deterministically named path under `$TMPDIR`
/// on Termux internal storage. Subsequent `open`/`stat`/`unlink` calls on the
/// original SDCARD path are intercepted by the corresponding hooks which
/// transparently redirect them to the same `$TMPDIR` path.
#[no_mangle]
pub unsafe extern "C" fn mkfifo(pathname: *const c_char, mode: libc::mode_t) -> libc::c_int {
    let orig_mkfifo = match ORIGINAL_MKFIFO.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("mkfifo") {
        return orig_mkfifo(pathname, mode);
    }

    let res = orig_mkfifo(pathname, mode);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::EACCES || err == libc::EPERM || err == libc::ENOSYS {
            let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
            if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
                let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
                let tmp_res = orig_mkfifo(tmp_path_c.as_ptr(), mode);
                if tmp_res == 0 {
                    log_trace!("[thook:mkfifo] ACTION: Redirected mkfifo '{}' to '{}'", pathname_str, tmp_path);
                    return 0;
                }
            }
        }
        // std::io::Error::from_raw_os_error(err);
    }
    res
}

/// Hook for `mkfifoat(3)`.
///
/// Same redirection logic as `mkfifo`. When the fallback tmp path is absolute,
/// `AT_FDCWD` is used for the redirected call because the dirfd is no longer
/// relevant.
#[no_mangle]
pub unsafe extern "C" fn mkfifoat(dirfd: libc::c_int, pathname: *const c_char, mode: libc::mode_t) -> libc::c_int {
    let orig_mkfifoat = match ORIGINAL_MKFIFOAT.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("mkfifoat") {
        return orig_mkfifoat(dirfd, pathname, mode);
    }

    let res = orig_mkfifoat(dirfd, pathname, mode);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::EACCES || err == libc::EPERM || err == libc::ENOSYS {
            let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
            if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
                let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
                // We drop dirfd in tmp because tmp_path is absolute anyway
                let tmp_res = orig_mkfifoat(libc::AT_FDCWD, tmp_path_c.as_ptr(), mode);
                if tmp_res == 0 {
                    log_trace!("[thook:mkfifoat] ACTION: Redirected mkfifoat '{}' to '{}'", pathname_str, tmp_path);
                    return 0;
                }
            }
        }
    }
    res
}

/// Hook for `link(2)`.
///
/// Hard links are not supported on the SDCARD FUSE filesystem. When `link`
/// fails with `EACCES`, `EPERM`, or `ENOSYS` for a path on SDCARD, the hook
/// falls back to a physical recursive copy. Hard link identity semantics
/// (shared inode) are lost but the copy ensures the caller does not abort.
#[no_mangle]
pub unsafe extern "C" fn link(oldpath: *const c_char, newpath: *const c_char) -> libc::c_int {
    let orig_link = match ORIGINAL_LINK.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("link") {
        return orig_link(oldpath, newpath);
    }

    let res = orig_link(oldpath, newpath);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::EACCES || err == libc::EPERM || err == libc::ENOSYS {
            let oldpath_str = if !oldpath.is_null() { CStr::from_ptr(oldpath).to_string_lossy().into_owned() } else { String::new() };
            let newpath_str = if !newpath.is_null() { CStr::from_ptr(newpath).to_string_lossy().into_owned() } else { String::new() };
            if is_sdcard_path(&newpath_str) {
                log_warn!("[thook:link] WARN: EACCES/EPERM on SDCARD for link '{}' -> '{}'. Engaging fallback physical copy (Hard link semantics lost!)", oldpath_str, newpath_str);
                if let Err(e) = copy_recursive(Path::new(&oldpath_str), Path::new(&newpath_str)) {
                    log_error!("[thook:link] ERROR: Fallback copy failed: {}", e);
                } else {
                    return 0;
                }
            }
        }
    }
    res
}

/// Hook for `linkat(2)`.
///
/// Same fallback copy logic as `link` for the directory-relative variant.
/// Paths are normalized to absolute before the copy.
#[no_mangle]
pub unsafe extern "C" fn linkat(olddirfd: libc::c_int, oldpath: *const c_char, newdirfd: libc::c_int, newpath: *const c_char, flags: libc::c_int) -> libc::c_int {
    let orig_linkat = match ORIGINAL_LINKAT.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("linkat") {
        return orig_linkat(olddirfd, oldpath, newdirfd, newpath, flags);
    }

    let res = orig_linkat(olddirfd, oldpath, newdirfd, newpath, flags);
    if res != 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if err == libc::EACCES || err == libc::EPERM || err == libc::ENOSYS {
            let oldpath_str = if !oldpath.is_null() { CStr::from_ptr(oldpath).to_string_lossy().into_owned() } else { String::new() };
            let newpath_str = if !newpath.is_null() { CStr::from_ptr(newpath).to_string_lossy().into_owned() } else { String::new() };
            if is_sdcard_path(&newpath_str) {
                log_warn!("[thook:linkat] WARN: EACCES/EPERM on SDCARD for linkat '{}' -> '{}'. Engaging fallback physical copy.", oldpath_str, newpath_str);
                if let Err(e) = copy_recursive(Path::new(&absolute_path(&oldpath_str)), Path::new(&absolute_path(&newpath_str))) {
                    log_error!("[thook:linkat] ERROR: Fallback copy failed: {}", e);
                } else {
                    return 0;
                }
            }
        }
    }
    res
}

// Macro that generates an `open`-family hook.
// On ENOENT, checks whether the path corresponds to a redirected FIFO in
// $TMPDIR and retries the open there if so. This makes named pipes created via
// the mkfifo hook transparently accessible from the original SDCARD path.
macro_rules! define_open_hook {
    ($fn_name:ident, $orig_name:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(pathname: *const c_char, flags: libc::c_int, mode: libc::mode_t) -> libc::c_int {
            let orig_fn = match $orig_name.get() {
                Some(f) => f,
                None => return -1,
            };

            if is_hook_disabled(stringify!($fn_name)) {
                return orig_fn(pathname, flags, mode);
            }

            let res = orig_fn(pathname, flags, mode);
            if res == -1 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err == libc::ENOENT {
                    let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
                    if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
                        let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
                        let fd = orig_fn(tmp_path_c.as_ptr(), flags, mode);
                        if fd != -1 {
                            log_trace!("[thook:open] ACTION: Redirected ENOENT '{}' to FIFO '{}'", pathname_str, tmp_path);
                            return fd;
                        }
                    }
                }
            }
            res
        }
    }
}

// Macro that generates an `openat`-family hook.
// Same FIFO-redirect logic as define_open_hook but for directory-relative opens.
// The dirfd argument is discarded for the fallback call because the tmp path is absolute.
macro_rules! define_openat_hook {
    ($fn_name:ident, $orig_name:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(dirfd: libc::c_int, pathname: *const c_char, flags: libc::c_int, mode: libc::mode_t) -> libc::c_int {
            let orig_fn = match $orig_name.get() {
                Some(f) => f,
                None => return -1,
            };

            if is_hook_disabled(stringify!($fn_name)) {
                return orig_fn(dirfd, pathname, flags, mode);
            }

            let res = orig_fn(dirfd, pathname, flags, mode);
            if res == -1 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err == libc::ENOENT {
                    let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
                    if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
                        let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
                        let fd = orig_fn(libc::AT_FDCWD, tmp_path_c.as_ptr(), flags, mode);
                        if fd != -1 {
                            log_trace!("[thook:openat] ACTION: Redirected ENOENT '{}' to FIFO '{}'", pathname_str, tmp_path);
                            return fd;
                        }
                    }
                }
            }
            res
        }
    }
}

// Macro that generates a stat-family hook.
// On ENOENT, checks whether the path has a FIFO counterpart in $TMPDIR and
// stats that instead. Allows callers to detect FIFO existence even when the
// original SDCARD path does not hold a real inode.
macro_rules! define_stat_hook {
    ($fn_name:ident, $orig_name:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(pathname: *const c_char, statbuf: *mut libc::c_void) -> libc::c_int {
            let orig_fn = match $orig_name.get() {
                Some(f) => f,
                None => return -1,
            };

            if is_hook_disabled(stringify!($fn_name)) {
                return orig_fn(pathname, statbuf);
            }

            let res = orig_fn(pathname, statbuf);
            if res == -1 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err == libc::ENOENT {
                    let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
                    if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
                        let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
                        let tmp_res = orig_fn(tmp_path_c.as_ptr(), statbuf);
                        if tmp_res == 0 {
                            log_trace!("[thook:stat] ACTION: Redirected ENOENT '{}' to FIFO '{}'", pathname_str, tmp_path);
                            return 0;
                        }
                    }
                }
            }
            res
        }
    }
}

define_open_hook!(open, ORIGINAL_OPEN);
define_open_hook!(open64, ORIGINAL_OPEN64);
define_openat_hook!(openat, ORIGINAL_OPENAT);
define_openat_hook!(openat64, ORIGINAL_OPENAT64);

define_stat_hook!(stat, ORIGINAL_STAT);
define_stat_hook!(lstat, ORIGINAL_LSTAT);
define_stat_hook!(stat64, ORIGINAL_STAT64);
define_stat_hook!(lstat64, ORIGINAL_LSTAT64);

/// Hook for `unlink(2)`.
///
/// After unlinking the original path (whatever the result), silently attempts
/// to unlink the corresponding FIFO tmp path if one exists. This prevents
/// orphaned pipe files in `$TMPDIR` when the caller removes an SDCARD path
/// that had a FIFO counterpart.
#[no_mangle]
pub unsafe extern "C" fn unlink(pathname: *const c_char) -> libc::c_int {
    let orig_unlink = match ORIGINAL_UNLINK.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("unlink") {
        return orig_unlink(pathname);
    }

    let res = orig_unlink(pathname);
    let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
    if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
        let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
        // Silently unlink the tmp pipe regardless of whether the SDCARD unlink succeeded.
        orig_unlink(tmp_path_c.as_ptr());
    }
    res
}

/// Hook for `unlinkat(2)`.
///
/// Same tmp-pipe cleanup as `unlink` for the directory-relative variant.
/// The dirfd is dropped for the tmp cleanup call because the tmp path is
/// always absolute and `AT_FDCWD` is correct.
#[no_mangle]
pub unsafe extern "C" fn unlinkat(dirfd: libc::c_int, pathname: *const c_char, flags: libc::c_int) -> libc::c_int {
    let orig_unlinkat = match ORIGINAL_UNLINKAT.get() {
        Some(f) => f,
        None => return -1,
    };

    if is_hook_disabled("unlinkat") {
        return orig_unlinkat(dirfd, pathname, flags);
    }

    let res = orig_unlinkat(dirfd, pathname, flags);
    let pathname_str = if !pathname.is_null() { CStr::from_ptr(pathname).to_string_lossy().into_owned() } else { String::new() };
    if let Some(tmp_path) = get_fifo_tmp_path(&pathname_str) {
        let tmp_path_c = CString::new(tmp_path.clone()).unwrap();
        orig_unlinkat(libc::AT_FDCWD, tmp_path_c.as_ptr(), flags);
    }
    res
}
