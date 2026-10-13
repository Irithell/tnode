use std::env;
use std::process::{Command, exit};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::Deserialize;

// Read from Cargo.toml at compile time so --tnode-version always matches the crate version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

// Embedded at compile-time for instant, zero-latency changelog rendering.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// List of valid syscall hook names supported by `libthook.so`.
const VALID_HOOK_NAMES: &[&str] = &[
    "realpath", "symlink", "symlinkat", "chmod", "fchmodat",
    "flock", "fcntl", "syscall", "execve", "execvp",
    "dlopen", "mkfifo", "mkfifoat",
    "open", "open64", "openat", "openat64",
    "stat", "lstat", "stat64", "lstat64",
    "link", "linkat", "unlink", "unlinkat",
    "readlink", "readlinkat",
];

macro_rules! log_err {
    ($($arg:tt)*) => {
        eprintln!("\x1b[31m[tnode:ERROR]\x1b[0m {}", format_args!($($arg)*))
    };
}

macro_rules! log_warn {
    ($($arg:tt)*) => {
        eprintln!("\x1b[33m[tnode:WARN]\x1b[0m {}", format_args!($($arg)*))
    };
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        println!("\x1b[36m[tnode:INFO]\x1b[0m {}", format_args!($($arg)*))
    };
}

/// Top-level configuration structure deserialized from `bridge.toml`.
///
/// `bridge.toml` is discovered by walking parent directories up to `$HOME` or `/sdcard`.
/// All fields default to empty/disabled so a missing or partial file never causes a panic.
#[derive(Deserialize, Default, Debug)]
struct BridgeConfig {
    #[serde(default)]
    core: CoreConfig,
    #[serde(default)]
    aliases: HashMap<String, String>,
    #[serde(default)]
    args: HashMap<String, Vec<String>>,
    #[serde(default)]
    env: HashMap<String, toml::Value>,
    #[serde(default)]
    hooks: HashMap<String, bool>,
}

/// Settings within the `[core]` section of `bridge.toml`.
///
/// All fields are `Option<T>` so that absent keys fall back to built-in
/// defaults rather than producing a deserialization error.
#[derive(Deserialize, Debug)]
struct CoreConfig {
    enabled: Option<bool>,
    tmp: Option<String>,
    lib: Option<String>,
    log: Option<String>,
    debug_level: Option<String>,
    #[allow(dead_code)]
    root: Option<bool>,
    hook_log: Option<String>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            tmp: None,
            lib: None,
            log: None,
            debug_level: None,
            root: None,
            hook_log: None,
        }
    }
}

/// Prints the help message to stdout and exits with code 0.
fn print_help() {
    println!(r#"
  _____                 _      
 |_   _|               | |     
   | |  _ __   ___   __| | ___ 
   | | | '_ \ / _ \ / _` |/ _ \
   | | | | | | (_) | (_| |  __/
   \_/ |_| |_|\___/ \__,_|\___|
                                
TNode - Termux Node.js Bridge (Smart Wrapper)
Version: {}

USAGE:
    tnode [TNODE_FLAGS] <COMMAND> [ARGS...]

DESCRIPTION:
    TNode injects libthook.so via LD_PRELOAD to bypass Android SDCARD restrictions.

TNODE FLAGS:
    --tnode-help                Print this help message and exit.
    --tnode-version             Print version information and exit.
    --tnode-update              Check and install the latest TNode release.
    --tnode-changelog           Display the version changelog panel.
    --tnode-stats               Show cache usage statistics grouped by file type.
    --tnode-completions <shell> Generate shell completion script (bash, zsh, fish).
    --tnode-lib <path>          Use a custom libthook.so path.
    --tnode-tmp <path>          Set a custom temporary directory for ELF caching.
    --tnode-log <path>          Output debug logs to the specified path.
    --tnode-debug <level>       Log level (off, error, warn, info, trace).
    --tnode-hook-log <hooks>    Force trace-level logs for specific hooks (e.g. dlopen,execve).
    --tnode-clean               Remove cached ELFs older than 24h.
    --tnode-clean-max <MB>      Remove cache files exceeding a size threshold.
    --tnode-init                Generate a default bridge.toml in the current directory.
    --tnode-bridge-off          Disable bridge.toml for this run.
    --tnode-bridge-on           Force enable bridge.toml for this run.
    --tnode-env                 Dump the active configuration.
    --tnode-env-export          Print shell export statements (eval compatible).
    --tnode-doctor              Run environment and system diagnostics.

EXAMPLES:
    tnode npm install canvas
    tnode --tnode-update
    tnode --tnode-changelog latest
    tnode --tnode-debug trace --tnode-log ./debug.log npx prisma db push
    tnode --tnode-hook-log dlopen,execve npm install
    tnode --tnode-clean
    tnode --tnode-clean-max 50
    tnode --tnode-stats
    eval "$(tnode --tnode-env-export)"
    tnode --tnode-completions bash >> ~/.bashrc
"#, VERSION);
}

/// Prints the version string to stdout.
fn print_version() {
    println!("tnode version {}", VERSION);
}

/// Returns the path to the user's tnode cache directory (`~/.cache/tnode`).
fn get_cache_dir() -> PathBuf {
    let base = env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/data/com.termux/files/home"));
    let dir = base.join(".cache").join("tnode");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Displays the changelog with colored indicators for Added (green), Changed (blue), Removed (red).
/// Accepts `latest` (default), `all`, or a specific version like `0.1.1` or `v0.1.1`.
fn print_changelog(target_version: Option<&str>) {
    let raw_target = target_version.unwrap_or("latest").trim_start_matches('v');
    let show_all = raw_target == "all";
    let is_latest = raw_target == "latest";

    let mut in_target = false;
    let mut found = false;
    let mut current_section = "";
    let mut last_printed_section = "";

    for line in CHANGELOG.lines() {
        if line.starts_with("## [") {
            if let Some(end_bracket) = line.find(']') {
                let ver = &line[4..end_bracket];
                let date = line.find(" - ").map(|idx| &line[idx + 3..]).unwrap_or("");
                current_section = "";
                last_printed_section = "";

                if is_latest {
                    if found { break; }
                    in_target = true;
                    found = true;
                    println!("\x1b[1mv{}\x1b[0m  \x1b[2m{}\x1b[0m", ver, date);
                    println!("\x1b[2m────────────────────────────────────────────────────\x1b[0m");
                } else if show_all {
                    if found { println!(); }
                    found = true;
                    in_target = true;
                    println!("\x1b[1mv{}\x1b[0m  \x1b[2m{}\x1b[0m", ver, date);
                    println!("\x1b[2m────────────────────────────────────────────────────\x1b[0m");
                } else {
                    if ver == raw_target {
                        if found { break; }
                        in_target = true;
                        found = true;
                        println!("\x1b[1mv{}\x1b[0m  \x1b[2m{}\x1b[0m", ver, date);
                        println!("\x1b[2m────────────────────────────────────────────────────\x1b[0m");
                    } else {
                        in_target = false;
                    }
                }
            }
            continue;
        }

        if !in_target { continue; }

        if line.starts_with("### ") {
            current_section = line[4..].trim();
            continue;
        }

        if line.starts_with("- ") {
            if !last_printed_section.is_empty() && last_printed_section != current_section {
                println!();
            }
            last_printed_section = current_section;
            let entry = line[2..].trim();
            match current_section {
                "Added"   => println!("  \x1b[32m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                "Changed" => println!("  \x1b[34m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                "Removed" => println!("  \x1b[31m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                _         => println!("  \x1b[37m{}\x1b[0m", entry),
            }
        }
    }

    if found {
        println!();
        println!("  \x1b[32m●\x1b[0m \x1b[2madded\x1b[0m   \x1b[34m●\x1b[0m \x1b[2mchanged\x1b[0m   \x1b[31m●\x1b[0m \x1b[2mremoved\x1b[0m");
        println!();
    } else {
        println!("[tnode] No changelog entry found for version '{}'", raw_target);
    }
}

/// Detects the target CPU architecture.
fn detect_arch() -> Result<String, String> {
    match env::consts::ARCH {
        "aarch64" => Ok("aarch64".to_string()),
        "x86_64"  => Ok("x86_64".to_string()),
        "arm"     => Ok("armv7".to_string()),
        "x86"     => Ok("i686".to_string()),
        other     => Err(format!("Unsupported architecture: {}", other)),
    }
}

/// Detects the target platform (android or linux).
fn detect_platform() -> Result<String, String> {
    match env::consts::OS {
        "linux" | "android" => {
            if Path::new("/data/data/com.termux").exists() || env::var("TERMUX_VERSION").is_ok() {
                Ok("android".to_string())
            } else {
                Ok("linux".to_string())
            }
        }
        "macos" | "darwin" => Err("macOS is not supported.".to_string()),
        other => Err(format!("Unsupported platform: {}", other)),
    }
}

/// Fetches the latest release tag_name from GitHub API via curl.
fn fetch_latest_release_tag() -> Option<String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--retry", "3",
               "https://api.github.com/repos/Irithell/tnode/releases/latest"])
        .output().ok()?;
    if !output.status.success() { return None; }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("\"tag_name\"") {
            if let Some(start) = line.find(':') {
                let rest = line[start + 1..].trim();
                let tag = rest.trim_matches(|c| c == '"' || c == ',' || c == ' ');
                if !tag.is_empty() { return Some(tag.to_string()); }
            }
        }
    }
    None
}

/// Returns true if `latest` is strictly newer than `current` according to semver (MAJOR.MINOR.PATCH).
fn is_version_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let clean = v.trim_start_matches('v');
        let mut parts = clean.split('.').filter_map(|s| s.parse::<u32>().ok());
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    };
    parse(latest) > parse(current)
}

/// Performs a self-update of tnode and libthook.so using atomic replacement.
/// Displays the changelog panel of the new version after successful installation.
fn handle_update() {
    println!("\x1b[36m[tnode]\x1b[0m Checking for updates...");
    let latest_tag = match fetch_latest_release_tag() {
        Some(tag) => tag,
        None => { log_err!("Failed to resolve latest version from GitHub API."); exit(1); }
    };
    let latest_ver = latest_tag.trim_start_matches('v');
    if !is_version_newer(&latest_ver, VERSION) {
        println!("\x1b[32m[tnode]\x1b[0m TNode is already up to date (v{}).", VERSION);
        exit(0);
    }
    println!("\x1b[36m[tnode]\x1b[0m Updating TNode v{} -> {}...", VERSION, latest_tag);
    let current_bin = match env::current_exe() {
        Ok(p) => p,
        Err(e) => { log_err!("Failed to resolve current binary path: {}", e); exit(1); }
    };
    let bin_dir = current_bin.parent().unwrap_or_else(|| Path::new("/data/data/com.termux/files/usr/bin"));
    let lib_dir = bin_dir.parent().map(|p| p.join("lib")).unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr/lib"));
    let target_lib = lib_dir.join("libthook.so");
    let arch = match detect_arch() { Ok(a) => a, Err(e) => { log_err!("{}", e); exit(1); } };
    let platform = match detect_platform() { Ok(p) => p, Err(e) => { log_err!("{}", e); exit(1); } };
    let bin_asset = format!("tnode-{}-{}", arch, platform);
    let lib_asset = format!("libthook-{}-{}.so", arch, platform);
    let base_url = format!("https://github.com/Irithell/tnode/releases/download/{}/", latest_tag);
    let tmp_dir = env::var("THOOK_TMPDIR").unwrap_or_else(|_| env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string()));
    let update_tmp = PathBuf::from(&tmp_dir).join("tnode_update_download");
    let _ = fs::create_dir_all(&update_tmp);
    let tmp_bin = update_tmp.join("tnode");
    let tmp_lib = update_tmp.join("libthook.so");
    println!("\x1b[36m[tnode]\x1b[0m Downloading {}...", bin_asset);
    let st = Command::new("curl").args(["-fsSL","--retry","3","--progress-bar","-o",tmp_bin.to_str().unwrap(),&format!("{}{}",base_url,bin_asset)]).status();
    if st.map(|s| !s.success()).unwrap_or(true) { log_err!("Failed to download {}", bin_asset); exit(1); }
    println!("\x1b[36m[tnode]\x1b[0m Downloading {}...", lib_asset);
    let st = Command::new("curl").args(["-fsSL","--retry","3","--progress-bar","-o",tmp_lib.to_str().unwrap(),&format!("{}{}",base_url,lib_asset)]).status();
    if st.map(|s| !s.success()).unwrap_or(true) { log_err!("Failed to download {}", lib_asset); exit(1); }
    let replace_file = |src: &Path, dst: &Path| -> Result<(), String> {
        let parent = dst.parent().ok_or_else(|| "Invalid destination".to_string())?;
        let _ = fs::create_dir_all(parent);
        let temp_dst = parent.join(format!(".tmp_replace_{}", dst.file_name().unwrap().to_string_lossy()));
        fs::copy(src, &temp_dst).map_err(|e| format!("Copy failed: {}", e))?;
        fs::rename(&temp_dst, dst).map_err(|e| { let _ = fs::remove_file(&temp_dst); format!("Rename failed: {}", e) })
    };
    println!("\x1b[36m[tnode]\x1b[0m Installing update...");
    if let Err(e) = replace_file(&tmp_bin, &current_bin) { log_err!("{}", e); exit(1); }
    use std::os::unix::fs::PermissionsExt;
    if let Ok(mut perms) = fs::metadata(&current_bin).map(|m| m.permissions()) {
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&current_bin, perms);
    }
    if let Err(e) = replace_file(&tmp_lib, &target_lib) { log_err!("{}", e); exit(1); }
    let _ = fs::remove_dir_all(&update_tmp);
    println!("\x1b[32m[tnode]\x1b[0m Successfully updated to {}!\n", latest_tag);

    let cache_dir = get_cache_dir();
    let _ = fs::write(cache_dir.join("latest_version"), &latest_tag);
    let _ = fs::write(
        cache_dir.join("last_check"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default(),
    );

    if Command::new(&current_bin).arg("--tnode-changelog").arg(&latest_ver).status().is_err() {
        print_changelog(Some(&latest_ver));
    }
}

/// Spawns a background thread to check for updates if 24h have passed since last check.
fn check_updates_background() {
    let cache_dir = get_cache_dir();
    let last_check_file = cache_dir.join("last_check");
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return,
    };
    let last_check: u64 = fs::read_to_string(&last_check_file).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    if now > last_check + 86400 {
        std::thread::spawn(move || {
            if let Some(tag) = fetch_latest_release_tag() {
                let cache_dir = get_cache_dir();
                let _ = fs::write(cache_dir.join("latest_version"), &tag);
                let _ = fs::write(cache_dir.join("last_check"), now.to_string());
            }
        });
    }
}

/// Prompts the user interactively if an update is available and the session is an interactive TTY.
/// Falls back to a compact notice if non-interactive or if the user declines.
fn check_and_prompt_update() {
    use std::io::{IsTerminal, Write};
    let cache_dir = get_cache_dir();
    let tag = match fs::read_to_string(cache_dir.join("latest_version")) {
        Ok(t) => t.trim().to_string(),
        Err(_) => return,
    };
    let ver = tag.trim_start_matches('v');
    if ver.is_empty() || !is_version_newer(ver, VERSION) {
        return;
    }

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprint!("\n\x1b[36m[tnode]\x1b[0m A new version is available: v{} -> {}\n", VERSION, tag);
        eprint!("\x1b[36m[tnode]\x1b[0m Would you like to update now? [y/N]: ");
        let _ = std::io::stderr().flush();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_lowercase();
            if trimmed == "y" || trimmed == "yes" {
                eprintln!("\x1b[36m[tnode]\x1b[0m Starting update...\n");
                handle_update();
                return;
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let _ = fs::write(cache_dir.join("last_check"), now);
    } else {
        eprintln!("\n\x1b[36m[tnode]\x1b[0m update available: v{} -> {}", VERSION, tag);
        eprintln!("\x1b[36m[tnode]\x1b[0m run: tnode --tnode-update\n");
    }
}

/// Searches for `tool` in every directory listed in PATH without relying on `which`.
fn find_in_path(tool: &str) -> Option<String> {
    let path_var = env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        if dir.is_empty() { continue; }
        let candidate = Path::new(dir).join(tool);
        if let Ok(meta) = fs::metadata(&candidate) {
            if meta.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 != 0 {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

/// Reads Android SDK API level from /system/build.prop. Returns None silently if inaccessible.
fn read_android_api_level() -> Option<u32> {
    let content = fs::read_to_string("/system/build.prop").ok()?;
    for line in content.lines() {
        if line.starts_with("ro.build.version.sdk=") {
            return line["ro.build.version.sdk=".len()..].trim().parse::<u32>().ok();
        }
    }
    None
}

/// Reads an Android system property via libc on Android or fallback to getprop.
fn get_android_prop(name: &str) -> Option<String> {
    #[cfg(target_os = "android")]
    unsafe {
        let c_name = std::ffi::CString::new(name).ok()?;
        let mut buf = [0 as libc::c_char; 92];
        let len = libc::__system_property_get(c_name.as_ptr(), buf.as_mut_ptr());
        if len > 0 {
            let val = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("/system/bin/getprop").arg(name).output() {
        if output.status.success() {
            let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

enum SelinuxState {
    Enforcing,
    Permissive,
    Inaccessible,
}

/// Inspects SELinux status using sysfs, system properties, and external getenforce.
fn get_selinux_status() -> (SelinuxState, &'static str) {
    if let Ok(val) = fs::read_to_string("/sys/fs/selinux/enforce") {
        if val.trim() == "1" {
            return (SelinuxState::Enforcing, "/sys/fs/selinux/enforce");
        } else if val.trim() == "0" {
            return (SelinuxState::Permissive, "/sys/fs/selinux/enforce");
        }
    }
    if let Some(boot) = get_android_prop("ro.boot.selinux") {
        if boot == "permissive" {
            return (SelinuxState::Permissive, "ro.boot.selinux");
        } else if boot == "enforcing" {
            return (SelinuxState::Enforcing, "ro.boot.selinux");
        }
    }
    if let Some(build) = get_android_prop("ro.build.selinux") {
        if build == "1" {
            return (SelinuxState::Enforcing, "ro.build.selinux");
        }
    }
    if let Ok(output) = std::process::Command::new("getenforce").output() {
        if output.status.success() {
            let out = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if out.contains("enforcing") {
                return (SelinuxState::Enforcing, "getenforce");
            } else if out.contains("permissive") || out.contains("disabled") {
                return (SelinuxState::Permissive, "getenforce");
            }
        }
    }
    (SelinuxState::Inaccessible, "SELinux policy restricts unprivileged reads")
}

/// Returns free space in bytes for the filesystem containing `path` using statvfs(2).
fn free_bytes_at(path: &str) -> Option<u64> {
    use std::mem::MaybeUninit;
    let c_path = std::ffi::CString::new(path).ok()?;
    unsafe {
        let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
        if libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) == 0 {
            let s = stat.assume_init();
            Some(s.f_bavail as u64 * s.f_frsize as u64)
        } else {
            None
        }
    }
}

/// Runs the `--tnode-doctor` diagnostic suite.
fn handle_doctor(config_path: Option<&PathBuf>, config: &BridgeConfig, lib_path: Option<&PathBuf>, resolved_tmp: &str) {
    println!("\x1b[35m=== TNode System & Environment Doctor ===\x1b[0m\n");
    let mut errors = 0;
    let mut warnings = 0;

    // libthook.so
    print!("Checking libthook.so: ");
    match lib_path {
        Some(p) if p.exists() => println!("\x1b[32m[OK]\x1b[0m Found at {}", p.display()),
        Some(p) => { println!("\x1b[31m[ERROR]\x1b[0m Resolved to {} but file does not exist.", p.display()); errors += 1; }
        None    => { println!("\x1b[31m[ERROR]\x1b[0m Could not locate libthook.so."); errors += 1; }
    }

    // TMPDIR writability
    print!("Checking Temporary Directory ({}): ", resolved_tmp);
    let tmp_path = Path::new(resolved_tmp);
    if tmp_path.exists() {
        let test_file = tmp_path.join(".tnode_doctor_test");
        if fs::write(&test_file, "test").is_ok() {
            let _ = fs::remove_file(test_file);
            println!("\x1b[32m[OK]\x1b[0m Writable.");
        } else {
            println!("\x1b[31m[ERROR]\x1b[0m Exists but NOT writable."); errors += 1;
        }
    } else if fs::create_dir_all(tmp_path).is_ok() {
        println!("\x1b[32m[OK]\x1b[0m Created successfully.");
    } else {
        println!("\x1b[31m[ERROR]\x1b[0m Failed to create directory."); errors += 1;
    }

    // Free disk space
    print!("Checking free disk space in TMPDIR: ");
    match free_bytes_at(resolved_tmp) {
        Some(bytes) => {
            let mb = bytes / (1024 * 1024);
            if mb < 50 {
                println!("\x1b[33m[WARN]\x1b[0m Only {}MB available. Cache operations may fail.", mb);
                warnings += 1;
            } else {
                println!("\x1b[32m[OK]\x1b[0m {}MB available.", mb);
            }
        }
        None => println!("\x1b[33m[SKIP]\x1b[0m Could not determine free space."),
    }

    // bridge.toml
    print!("Checking bridge.toml: ");
    match config_path {
        Some(p) if p.exists() => println!("\x1b[32m[OK]\x1b[0m Found at {}", p.display()),
        _ => println!("\x1b[33m[INFO]\x1b[0m No bridge.toml found in directory tree."),
    }

    // Primary runtime check
    println!("\n\x1b[36mChecking runtime tools in PATH:\x1b[0m");
    print!("  Node.js runtime: ");
    match find_in_path("node") {
        Some(p) => println!("\x1b[32m[FOUND]\x1b[0m at {}", p),
        None => {
            println!("\x1b[33m[NOT FOUND]\x1b[0m (install via 'pkg install nodejs')");
            warnings += 1;
        }
    }
    if !config.aliases.is_empty() {
        println!("\n\x1b[36mChecking configured aliases from bridge.toml:\x1b[0m");
        for (alias, target) in &config.aliases {
            print!("  alias {:<12} -> {:<10}: ", alias, target);
            match find_in_path(target) {
                Some(p) => println!("\x1b[32m[FOUND]\x1b[0m at {}", p),
                None => println!("\x1b[33m[NOT FOUND in PATH]\x1b[0m"),
            }
        }
    }

    // CWD filesystem
    println!("\n\x1b[36mChecking Current Working Directory:\x1b[0m");
    if let Ok(cwd) = env::current_dir() {
        let cwd_str = cwd.to_string_lossy();
        print!("  CWD: {} ", cwd_str);
        if is_sdcard_path(&cwd_str) {
            println!("\x1b[33m[SDCARD/FUSE Mount Detected]\x1b[0m");
            println!("  -> libthook.so interception active for this session.");
        } else {
            println!("\x1b[32m[Internal Termux Filesystem]\x1b[0m");
        }
    }

    // Android environment
    println!("\n\x1b[36mChecking Android environment:\x1b[0m");
    if let Some(api) = read_android_api_level() {
        print!("  Android API level: {}", api);
        if api >= 31 {
            println!("  \x1b[33m[WARN]\x1b[0m Android 12+ (API {}). Phantom Process Killer is active.", api);
            println!("         Background worker threads spawned by npm may be SIGKILL'd.");
            warnings += 1;
        } else {
            println!("  \x1b[32m[OK]\x1b[0m");
        }
    }

    // SELinux
    print!("  SELinux status: ");
    let (state, source) = get_selinux_status();
    match state {
        SelinuxState::Enforcing => {
            println!("\x1b[33m[WARN]\x1b[0m Enforcing (via {}). Some ROMs may restrict LD_PRELOAD in certain contexts.", source);
            warnings += 1;
        }
        SelinuxState::Permissive => {
            println!("\x1b[32m[Permissive or disabled]\x1b[0m (via {})", source);
        }
        SelinuxState::Inaccessible => {
            println!("\x1b[36m[Inaccessible / Sandboxed]\x1b[0m ({})", source);
        }
    }

    println!("\nDiagnostics complete: {} error(s), {} warning(s).", errors, warnings);
    if errors > 0 { exit(1); }
}

/// Displays a terminal table summarizing cache usage in `$TMPDIR` grouped by file prefix.
fn handle_stats(tmp_dir: &str) {
    let path = Path::new(tmp_dir);
    if !path.exists() {
        log_info!("TMPDIR '{}' does not exist. No cache to report.", tmp_dir);
        return;
    }
    let read_dir = match fs::read_dir(path) {
        Ok(d) => d,
        Err(e) => { log_err!("Failed to read '{}': {}", tmp_dir, e); return; }
    };

    struct Group { count: usize, total_bytes: u64, oldest_secs: u64 }
    let mut groups: HashMap<&str, Group> = HashMap::new();
    groups.insert("thook_exec_*", Group { count: 0, total_bytes: 0, oldest_secs: 0 });
    groups.insert("thook_dl_*",   Group { count: 0, total_bytes: 0, oldest_secs: 0 });
    groups.insert("tnode_exec_*", Group { count: 0, total_bytes: 0, oldest_secs: 0 });

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let prefix = if name.starts_with("thook_exec_") { "thook_exec_*" }
                     else if name.starts_with("thook_dl_")   { "thook_dl_*" }
                     else if name.starts_with("tnode_exec_") { "tnode_exec_*" }
                     else { continue };
        if let Ok(meta) = entry.metadata() {
            let g = groups.get_mut(prefix).unwrap();
            g.count += 1;
            g.total_bytes += meta.len();
            if let Ok(modified) = meta.modified() {
                if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let age = now_secs.saturating_sub(dur.as_secs());
                    if g.oldest_secs == 0 || age > g.oldest_secs { g.oldest_secs = age; }
                }
            }
        }
    }

    println!("\x1b[35m=== TNode Cache Statistics ===\x1b[0m");
    println!("Directory: {}\n", tmp_dir);
    println!("  {:<16} {:>6}  {:>10}  {}", "Type", "Files", "Size", "Oldest");
    println!("  {}", "─".repeat(52));
    for (prefix, g) in &groups {
        let size_str = if g.total_bytes >= 1024*1024 { format!("{:.1}MB", g.total_bytes as f64/1048576.0) }
                       else if g.total_bytes >= 1024  { format!("{:.1}KB", g.total_bytes as f64/1024.0) }
                       else                           { format!("{}B", g.total_bytes) };
        let age_str = if g.count == 0           { "—".to_string() }
                      else if g.oldest_secs >= 3600 { format!("{}h ago", g.oldest_secs/3600) }
                      else                           { format!("{}m ago", g.oldest_secs/60) };
        println!("  {:<16} {:>6}  {:>10}  {}", prefix, g.count, size_str, age_str);
    }
    println!();
}

/// Outputs shell completion scripts for `bash`, `zsh`, or `fish`.
fn print_completions(shell: &str) {
    match shell {
        "bash" => println!(r#"# tnode bash completion
# Add to ~/.bashrc:  source <(tnode --tnode-completions bash)
_tnode_completions() {{
    local cur prev
    _init_completion || return
    local tnode_flags="--tnode-help --tnode-version --tnode-update --tnode-changelog
        --tnode-stats --tnode-completions --tnode-lib --tnode-tmp --tnode-log
        --tnode-debug --tnode-hook-log --tnode-clean --tnode-clean-max --tnode-init
        --tnode-bridge-off --tnode-bridge-on --tnode-env --tnode-env-export --tnode-doctor"
    case "$prev" in
        --tnode-debug)
            COMPREPLY=( $(compgen -W "off error warn info trace" -- "$cur") ); return ;;
        --tnode-changelog)
            COMPREPLY=( $(compgen -W "latest all" -- "$cur") ); return ;;
        --tnode-completions)
            COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") ); return ;;
        --tnode-lib|--tnode-tmp|--tnode-log)
            COMPREPLY=( $(compgen -f -- "$cur") ); return ;;
        --tnode-clean-max)
            COMPREPLY=(); return ;;
        --tnode-hook-log)
            COMPREPLY=( $(compgen -W "realpath symlink symlinkat chmod fchmodat flock fcntl syscall execve execvp dlopen mkfifo mkfifoat open open64 openat openat64 stat lstat stat64 lstat64 link linkat unlink unlinkat readlink readlinkat" -- "$cur") )
            return ;;
    esac
    if [[ "$cur" == --tnode-* ]]; then
        COMPREPLY=( $(compgen -W "$tnode_flags" -- "$cur") )
    else
        COMPREPLY=( $(compgen -c -- "$cur") )
    fi
}}
complete -F _tnode_completions tnode"#),
        "zsh" => println!(r#"#compdef tnode
# tnode zsh completion
# Add to ~/.zshrc:  source <(tnode --tnode-completions zsh)
_tnode() {{
    local -a tnode_flags
    tnode_flags=(
        '--tnode-help[Print help and exit]'
        '--tnode-version[Print version and exit]'
        '--tnode-update[Download and install latest release]'
        '--tnode-changelog[Display changelog]:version:(latest all)'
        '--tnode-stats[Show cache usage statistics]'
        '--tnode-completions[Generate shell completions]:shell:(bash zsh fish)'
        '--tnode-lib[Custom libthook.so path]:path:_files'
        '--tnode-tmp[Custom ELF cache directory]:path:_files -/'
        '--tnode-log[Log file path]:path:_files'
        '--tnode-debug[Log level]:level:(off error warn info trace)'
        '--tnode-hook-log[Force trace for specific hooks]:hooks:(realpath symlink execve dlopen mkfifo flock)'
        '--tnode-clean[Remove cache files older than 24h]'
        '--tnode-clean-max[Remove cache files exceeding size]:megabytes:'
        '--tnode-init[Generate default bridge.toml]'
        '--tnode-bridge-off[Disable bridge.toml for this run]'
        '--tnode-bridge-on[Force enable bridge.toml]'
        '--tnode-env[Dump active configuration]'
        '--tnode-env-export[Print export statements]'
        '--tnode-doctor[Run system diagnostics]'
    )
    _arguments -C $tnode_flags '*:command:_command_names'
}}
_tnode"#),
        "fish" => println!(r#"# tnode fish completion
# tnode --tnode-completions fish > ~/.config/fish/completions/tnode.fish
complete -c tnode -l tnode-help        -d 'Print help and exit'
complete -c tnode -l tnode-version     -d 'Print version and exit'
complete -c tnode -l tnode-update      -d 'Download and install latest release'
complete -c tnode -l tnode-changelog   -d 'Display changelog' -r -a 'latest all'
complete -c tnode -l tnode-stats       -d 'Show cache usage statistics'
complete -c tnode -l tnode-completions -d 'Generate shell completions' -r -a 'bash zsh fish'
complete -c tnode -l tnode-lib         -d 'Custom libthook.so path' -r -F
complete -c tnode -l tnode-tmp         -d 'Custom ELF cache directory' -r -F
complete -c tnode -l tnode-log         -d 'Log file path' -r -F
complete -c tnode -l tnode-debug       -d 'Log level' -r -a 'off error warn info trace'
complete -c tnode -l tnode-hook-log    -d 'Force trace for specific hooks' -r -a 'realpath symlink symlinkat chmod fchmodat flock fcntl syscall execve execvp dlopen mkfifo mkfifoat open openat stat lstat link linkat unlink unlinkat readlink readlinkat'
complete -c tnode -l tnode-clean       -d 'Remove cache files older than 24h'
complete -c tnode -l tnode-clean-max   -d 'Remove cache files exceeding size (MB)' -r
complete -c tnode -l tnode-init        -d 'Generate default bridge.toml'
complete -c tnode -l tnode-bridge-off  -d 'Disable bridge.toml for this run'
complete -c tnode -l tnode-bridge-on   -d 'Force enable bridge.toml'
complete -c tnode -l tnode-env         -d 'Dump active configuration'
complete -c tnode -l tnode-env-export  -d 'Print export statements'
complete -c tnode -l tnode-doctor      -d 'Run system diagnostics'"#),
        other => { log_err!("Unknown shell '{}'. Supported: bash, zsh, fish.", other); exit(1); }
    }
}

/// Validates hook names from a comma-separated string. Warns and strips invalid names.
fn validate_hook_names(raw: &str) -> Option<String> {
    let valid: Vec<&str> = raw.split(',').map(|s| s.trim()).filter(|name| {
        if name.is_empty() { return false; }
        if VALID_HOOK_NAMES.contains(name) { true } else {
            log_warn!("Unknown hook name '{}' — ignored. Valid names: {}", name, VALID_HOOK_NAMES.join(", "));
            false
        }
    }).collect();
    if valid.is_empty() { None } else { Some(valid.join(",")) }
}

/// Returns the absolute form of `path`, joining to CWD if relative.
fn absolute_path(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() { return path.to_string(); }
    match env::current_dir() {
        Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
        Err(_)  => path.to_string(),
    }
}

/// Walks parent directories from CWD looking for bridge.toml.
/// Stops at $HOME, /storage, /sdcard roots, or filesystem root.
fn find_bridge_toml() -> Option<PathBuf> {
    let home = env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".to_string());
    let home_path = PathBuf::from(&home);
    let mut current = env::current_dir().ok()?;
    loop {
        let candidate = current.join("bridge.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        let cur_str = current.to_string_lossy().to_string();
        if cur_str == "/storage" || cur_str == "/sdcard" || cur_str == "/" { break; }
        if current == home_path { break; }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None    => break,
        }
    }
    None
}

/// Searches for `libthook.so` in system paths.
fn find_thook_lib() -> Option<PathBuf> {
    if let Ok(p) = env::var("THOOK_LIB_PATH") {
        let path = PathBuf::from(p);
        if path.exists() { return Some(path); }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            for rel in &["../lib/libthook.so", "libthook.so",
                         "../../target/debug/libthook.so", "../../target/release/libthook.so"] {
                let p = parent.join(rel);
                if p.exists() { return Some(p); }
            }
        }
    }
    None
}

/// Removes cached ELF and shared library files older than 24 hours from `tmp_dir`.
fn handle_clean(tmp_dir: &str) {
    let path = Path::new(tmp_dir);
    if !path.exists() { log_info!("TMPDIR '{}' does not exist. Nothing to clean.", tmp_dir); return; }
    let read_dir = match fs::read_dir(path) {
        Ok(d) => d,
        Err(e) => { log_err!("Failed to read '{}': {}", tmp_dir, e); return; }
    };
    let now = std::time::SystemTime::now();
    let ttl = std::time::Duration::from_secs(86400);
    let mut cleaned = 0;
    let mut skipped = 0;
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("tnode_exec_") || name.starts_with("thook_dl_") || name.starts_with("thook_exec_") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > ttl {
                            if fs::remove_file(entry.path()).is_ok() { cleaned += 1; log_info!("Cleaned: {}", name); }
                        } else { skipped += 1; }
                    }
                }
            }
        }
    }
    log_info!("Garbage collection complete: {} cleaned, {} active.", cleaned, skipped);
}

/// Removes cached ELF and shared library files exceeding `max_mb` in size.
fn handle_clean_max(tmp_dir: &str, max_mb: u64) {
    let path = Path::new(tmp_dir);
    if !path.exists() { log_info!("TMPDIR '{}' does not exist. Nothing to clean.", tmp_dir); return; }
    let read_dir = match fs::read_dir(path) {
        Ok(d) => d,
        Err(e) => { log_err!("Failed to read '{}': {}", tmp_dir, e); return; }
    };
    let max_bytes = max_mb * 1024 * 1024;
    let mut cleaned = 0;
    let mut skipped = 0;
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("tnode_exec_") || name.starts_with("thook_dl_") || name.starts_with("thook_exec_") {
            if let Ok(meta) = entry.metadata() {
                if meta.len() > max_bytes {
                    let size_mb = meta.len() as f64 / 1048576.0;
                    if fs::remove_file(entry.path()).is_ok() {
                        cleaned += 1;
                        log_info!("Cleaned oversized ({:.1}MB): {}", size_mb, name);
                    }
                } else { skipped += 1; }
            }
        }
    }
    log_info!("Size-based cleanup complete: {} cleaned, {} kept.", cleaned, skipped);
}

/// Writes a default `bridge.toml` to `config_path` if it does not already exist.
fn generate_default_config(config_path: &Path) {
    if config_path.exists() { return; }
    let default_tmp = env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string());
    let default_node_path = find_in_path("node").unwrap_or_else(|| "/data/data/com.termux/files/usr/bin/node".to_string());
    let content = format!(r#"# bridge.toml — TNode project-level configuration
[core]
enabled = true
tmp = "{}"
lib = "/data/data/com.termux/files/usr/lib/libthook.so"
# log = "thook-{{timestamp}}.log"
debug_level = "off"
# hook_log = "dlopen,execve"    # force trace-level logs for specific hooks

[aliases]
node = "{}"

[args]
# npm = ["--ignore-scripts"]

[env]
# PRISMA_QUERY_ENGINE_LIBRARY = "/path/to/libquery_engine.so.node"

[hooks]
# All hooks are enabled by default. Set to false to disable a specific hook.
# Valid names: realpath, symlink, symlinkat, chmod, fchmodat, flock, fcntl,
#              syscall, execve, execvp, dlopen, mkfifo, mkfifoat, open, open64,
#              openat, openat64, stat, lstat, stat64, lstat64, link, linkat,
#              unlink, unlinkat, readlink, readlinkat
# realpath = false
# dlopen = false
"#, default_tmp, default_node_path);
    if fs::write(config_path, content).is_ok() {
        log_info!("Generated default bridge.toml at {}", config_path.display());
    }
}

/// Returns true if `path` resides on an Android SDCARD / shared external storage mount.
fn is_sdcard_path(path: &str) -> bool {
    if path.starts_with("/storage/")
        || path.starts_with("/sdcard/")
        || path == "/sdcard"
        || path.starts_with("/mnt/sdcard/")
        || path.starts_with("/mnt/media_rw/")
    {
        return true;
    }
    if let Ok(canon) = fs::canonicalize(path) {
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

/// Validates whether a temporary directory path is suitable for ELF caching and FIFO redirection.
///
/// Returns `false` if the path:
/// 1. Is located on an SDCARD / FUSE mount (`/storage/` or `/sdcard/`).
/// 2. Cannot be created or is not writable.
fn is_suitable_tmp_dir(path_str: &str) -> bool {
    if is_sdcard_path(path_str) { return false; }
    let p = Path::new(path_str);
    if fs::create_dir_all(p).is_err() { return false; }
    let test_file = p.join(".tnode_writable_test");
    if fs::write(&test_file, "test").is_ok() { let _ = fs::remove_file(test_file); true } else { false }
}

fn main() {
    let raw_args: Vec<String> = env::args().collect();
    let mut iter = raw_args.into_iter().skip(1);

    let mut target_command = None;
    let mut target_args = Vec::new();
    let mut custom_lib = None;
    let mut inject_debug = None;
    let mut log_file = None;
    let mut custom_tmp = None;
    let mut should_clean = false;
    let mut clean_max_mb: Option<u64> = None;
    let mut bridge_off = false;
    let mut bridge_on = false;
    let mut show_env = false;
    let mut show_env_export = false;
    let mut run_doctor = false;
    let mut should_init = false;
    let mut show_changelog = false;
    let mut changelog_target = None;
    let mut run_update = false;
    let mut run_stats = false;
    let mut run_completions: Option<String> = None;
    let mut hook_log_cli: Option<String> = None;

    while let Some(arg) = iter.next() {
        if arg.starts_with("--tnode-") {
            match arg.as_str() {
                "--tnode-help"        => { print_help(); exit(0); }
                "--tnode-version"     => { print_version(); exit(0); }
                "--tnode-clean"       => should_clean = true,
                "--tnode-stats"       => run_stats = true,
                "--tnode-bridge-off"  => bridge_off = true,
                "--tnode-bridge-on"   => bridge_on = true,
                "--tnode-env"         => show_env = true,
                "--tnode-env-export"  => show_env_export = true,
                "--tnode-doctor"      => run_doctor = true,
                "--tnode-init"        => should_init = true,
                "--tnode-update"      => run_update = true,
                "--tnode-changelog" => {
                    show_changelog = true;
                    if let Some(val) = iter.next() {
                        if !val.starts_with("--tnode-") && (val == "latest" || val == "all" || val.starts_with('v') || val.chars().next().map_or(false, |c| c.is_ascii_digit())) {
                            changelog_target = Some(val);
                        } else {
                            target_command = Some(val);
                            target_args.extend(iter);
                            break;
                        }
                    }
                }
                "--tnode-completions" => {
                    let shell = match iter.next() {
                        Some(v) if !v.starts_with("--tnode-") => v,
                        _ => { log_err!("Missing argument for --tnode-completions <bash|zsh|fish>"); exit(1); }
                    };
                    run_completions = Some(shell);
                }
                "--tnode-clean-max" => {
                    let val = match iter.next() {
                        Some(v) if !v.starts_with("--tnode-") => v,
                        _ => { log_err!("Missing argument for --tnode-clean-max <MB>"); exit(1); }
                    };
                    match val.parse::<u64>() {
                        Ok(mb) => clean_max_mb = Some(mb),
                        Err(_) => { log_err!("Invalid value for --tnode-clean-max: '{}'. Expected MB.", val); exit(1); }
                    }
                }
                "--tnode-hook-log" => {
                    let val = match iter.next() {
                        Some(v) if !v.starts_with("--tnode-") => v,
                        _ => { log_err!("Missing argument for --tnode-hook-log <hook[,hook,...]>"); exit(1); }
                    };
                    hook_log_cli = Some(val);
                }
                "--tnode-debug" => {
                    let next_val = match iter.next() {
                        Some(v) if !v.starts_with("--tnode-") => v,
                        _ => { log_err!("Missing argument for '{}'", arg); exit(1); }
                    };
                    inject_debug = Some(next_val);
                }
                "--tnode-lib" | "--tnode-log" | "--tnode-tmp" => {
                    let next_val = match iter.next() {
                        Some(v) if !v.starts_with("--tnode-") => v,
                        _ => { log_err!("Missing argument for '{}'", arg); exit(1); }
                    };
                    match arg.as_str() {
                        "--tnode-lib" => custom_lib = Some(next_val),
                        "--tnode-log" => log_file = Some(absolute_path(&next_val)),
                        "--tnode-tmp" => custom_tmp = Some(absolute_path(&next_val)),
                        _ => unreachable!(),
                    }
                }
                _ => { log_err!("Unknown flag '{}'", arg); exit(1); }
            }
        } else {
            target_command = Some(arg);
            target_args.extend(iter);
            break;
        }
    }

    if show_changelog { print_changelog(changelog_target.as_deref()); exit(0); }
    if run_update { handle_update(); exit(0); }
    if let Some(ref shell) = run_completions { print_completions(shell); exit(0); }

    let bridge_toml_path = find_bridge_toml();

    if should_init {
        generate_default_config(&PathBuf::from("./bridge.toml"));
        exit(0);
    }

    let mut config = BridgeConfig::default();
    if let Some(ref path) = bridge_toml_path {
        if let Ok(content) = fs::read_to_string(path) {
            match toml::from_str::<BridgeConfig>(&content) {
                Ok(parsed) => config = parsed,
                Err(e)     => log_warn!("Failed to parse bridge.toml at {}: {}", path.display(), e),
            }
        }
    }

    if bridge_off { config.core.enabled = Some(false); }
    else if bridge_on { config.core.enabled = Some(true); }

    let is_enabled = config.core.enabled.unwrap_or(true);

    let candidate_lib = custom_lib.map(PathBuf::from).or_else(|| config.core.lib.as_ref().map(PathBuf::from));
    let resolved_lib_path = match candidate_lib {
        Some(path) if path.exists() && path.is_file() => Some(path),
        Some(path) => {
            log_warn!("Configured libthook.so '{}' is invalid. Falling back to auto-discovery...", path.display());
            find_thook_lib()
        }
        None => find_thook_lib(),
    };

    let default_tmp = env::var("THOOK_TMPDIR")
        .unwrap_or_else(|_| env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string()));
    let candidate_tmp = custom_tmp.or(config.core.tmp.clone());
    let resolved_tmp = match candidate_tmp {
        Some(custom) => {
            if is_suitable_tmp_dir(&custom) { custom } else {
                log_warn!("Custom tmp '{}' is invalid or on SDCARD. Falling back to: {}", custom, default_tmp);
                if is_suitable_tmp_dir(&default_tmp) { default_tmp } else { "/data/data/com.termux/files/usr/tmp".to_string() }
            }
        }
        None => if is_suitable_tmp_dir(&default_tmp) { default_tmp } else { "/data/data/com.termux/files/usr/tmp".to_string() }
    };

    if run_stats { handle_stats(&resolved_tmp); exit(0); }

    if should_clean { handle_clean(&resolved_tmp); }
    if let Some(max_mb) = clean_max_mb { handle_clean_max(&resolved_tmp, max_mb); }
    if should_clean || clean_max_mb.is_some() { exit(0); }

    if run_doctor {
        handle_doctor(bridge_toml_path.as_ref(), &config, resolved_lib_path.as_ref(), &resolved_tmp);
        exit(0);
    }

    if show_env {
        println!("=== TNode Bridge Active Configuration ===");
        println!("Core Enabled     : {}", is_enabled);
        println!("Resolved Lib     : {}", resolved_lib_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "NOT FOUND".to_string()));
        println!("Resolved TmpDir  : {}", resolved_tmp);
        println!("Debug Level      : {}", inject_debug.as_ref().or(config.core.debug_level.as_ref()).unwrap_or(&"off".to_string()));
        println!("Log File         : {}", log_file.as_ref().or(config.core.log.as_ref()).unwrap_or(&"stderr".to_string()));
        if !config.hooks.is_empty() {
            println!("\n[Hooks]");
            for (k, v) in &config.hooks { println!("  {:<16} = {}", k, if *v { "enabled" } else { "disabled" }); }
        }
        println!("\n[Aliases]");
        for (k, v) in &config.aliases { println!("  {:<12} -> {}", k, v); }
        println!("\n[Prepended Args]");
        for (k, v) in &config.args { println!("  {:<12} -> {:?}", k, v); }
        println!("\n[Injected Env]");
        for (k, v) in &config.env { println!("  {:<12} = {}", k, v); }
        exit(0);
    }

    let lib_path = match resolved_lib_path {
        Some(p) => p,
        None    => { log_err!("Could not locate libthook.so. Install it or set THOOK_LIB_PATH."); exit(1); }
    };

    if show_env_export {
        println!("export LD_PRELOAD=\"{}\"", lib_path.display());
        println!("export THOOK_TMPDIR=\"{}\"", resolved_tmp);
        if let Some(ref dbg) = inject_debug.or(config.core.debug_level) { println!("export THOOK_DEBUG_LEVEL=\"{}\"", dbg); }
        if let Some(ref log) = log_file.or(config.core.log) { println!("export THOOK_LOG_FILE=\"{}\"", log); }
        for (k, v) in &config.env {
            println!("export {}=\"{}\"", k, v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()));
        }
        exit(0);
    }

    let (raw_bin, user_args) = match target_command {
        Some(bin) => (bin, target_args),
        None      => { print_help(); exit(0); }
    };

    let bin = if is_enabled { config.aliases.get(&raw_bin).cloned().unwrap_or(raw_bin) } else { raw_bin };

    let mut final_args = Vec::new();
    if is_enabled { if let Some(extra) = config.args.get(&bin) { final_args.extend(extra.clone()); } }
    final_args.extend(user_args);

    let debug_val = inject_debug.or(config.core.debug_level).unwrap_or_else(|| "off".to_string());
    let log_val = log_file.or(config.core.log);

    // Build THOOK_DISABLE_HOOKS from [hooks] section
    let disable_hooks_val: Option<String> = if is_enabled && !config.hooks.is_empty() {
        let disabled: Vec<&str> = config.hooks.iter()
            .filter(|(_, &enabled)| !enabled)
            .filter_map(|(name, _)| {
                if VALID_HOOK_NAMES.contains(&name.as_str()) { Some(name.as_str()) }
                else { log_warn!("Unknown hook '{}' in [hooks] — ignored.", name); None }
            }).collect();
        if disabled.is_empty() { None } else { Some(disabled.join(",")) }
    } else { None };

    // Resolve THOOK_LOG_HOOKS: CLI flag takes precedence over bridge.toml hook_log
    let log_hooks_val: Option<String> = hook_log_cli.as_deref()
        .or(config.core.hook_log.as_deref())
        .and_then(|raw| validate_hook_names(raw));

    check_updates_background();

    let attempt_execution = |bin: &str, args: &[String]| -> std::io::Result<std::process::ExitStatus> {
        let mut cmd = Command::new(bin);
        cmd.args(args);
        cmd.env("LD_PRELOAD", lib_path.to_string_lossy().to_string());
        for (k, v) in &config.env {
            cmd.env(k, v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()));
        }
        let b = bin.to_string();
        if b.contains("npm") || b.contains("npx") || b.contains("yarn") || b.contains("pnpm") || b.contains("node-gyp") {
            cmd.env("npm_config_android_ndk_path", "dummy");
        }
        cmd.env("THOOK_TMPDIR", &resolved_tmp);
        cmd.env("THOOK_DEBUG_LEVEL", &debug_val);
        if let Some(ref log_path) = log_val {
            let expanded = log_path
                .replace("{timestamp}", &std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs().to_string()).unwrap_or_default())
                .replace("{pid}", &std::process::id().to_string());
            cmd.env("THOOK_LOG_FILE", expanded);
        }
        if let Some(ref disabled) = disable_hooks_val { cmd.env("THOOK_DISABLE_HOOKS", disabled); }
        if let Some(ref log_hooks) = log_hooks_val    { cmd.env("THOOK_LOG_HOOKS", log_hooks); }

        // Guarantee $PREFIX/bin is present in PATH for child processes
        let prefix = env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
        let prefix_bin = format!("{}/bin", prefix);
        let current_path = env::var("PATH").unwrap_or_default();
        if !current_path.split(':').any(|p| p == prefix_bin) {
            let new_path = if current_path.is_empty() { prefix_bin } else { format!("{}:{}", prefix_bin, current_path) };
            cmd.env("PATH", new_path);
        }

        cmd.status()
    };

    // Support direct execution of scripts (e.g. `tnode start.sh`, `tnode ./start.sh`)
    let resolved_script_path = if Path::new(&bin).is_file() {
        Some(bin.clone())
    } else if !bin.contains('/') && Path::new(&format!("./{}", bin)).is_file() {
        Some(format!("./{}", bin))
    } else {
        None
    };

    let result = if let Some(ref script_file) = resolved_script_path {
        if is_elf(script_file) {
            attempt_execution(script_file, &final_args)
        } else if let Some(shebang_args) = get_shebang(script_file) {
            let mut fallback_args = shebang_args;
            fallback_args.push(script_file.clone());
            fallback_args.extend(final_args.clone());
            let interp = fallback_args.remove(0);
            attempt_execution(&interp, &fallback_args)
        } else if script_file.ends_with(".sh") || !is_elf(script_file) {
            let prefix = env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
            let sh_path = format!("{}/bin/sh", prefix);
            let interp = if Path::new(&sh_path).exists() { sh_path } else { "/bin/sh".to_string() };
            let mut fallback_args = vec![script_file.clone()];
            fallback_args.extend(final_args.clone());
            attempt_execution(&interp, &fallback_args)
        } else {
            attempt_execution(script_file, &final_args)
        }
    } else {
        attempt_execution(&bin, &final_args)
    };

    match result {
        Ok(status) => { check_and_prompt_update(); exit(status.code().unwrap_or(0)); }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                log_warn!("EACCES executing '{}'. Attempting fallback...", bin);
                if let Some(shebang_args) = get_shebang(&bin) {
                    let mut fallback_args = shebang_args;
                    fallback_args.push(bin.clone());
                    fallback_args.extend(final_args.clone());
                    let interp = fallback_args.remove(0);
                    if let Ok(st) = attempt_execution(&interp, &fallback_args) { check_and_prompt_update(); exit(st.code().unwrap_or(0)); }
                } else if is_elf(&bin) {
                    if let Some(copied) = copy_elf_to_tmp(&bin, &resolved_tmp) {
                        if let Ok(st) = attempt_execution(&copied, &final_args) { check_and_prompt_update(); exit(st.code().unwrap_or(0)); }
                    }
                }
            }
            log_err!("Failed to execute '{}': {}", bin, e);
            check_and_prompt_update();
            exit(1);
        }
    }
}

/// Reads the shebang line from `filepath` and returns the interpreter path and
/// any arguments embedded in the shebang line.
fn get_shebang(filepath: &str) -> Option<Vec<String>> {
    use std::io::Read;
    let mut file = fs::File::open(filepath).ok()?;
    let mut buffer = [0u8; 256];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);
    if bytes_read < 2 { return None; }
    if buffer[0] == b'#' && buffer[1] == b'!' {
        let shebang_line: Vec<u8> = buffer.iter().take(bytes_read).take_while(|&&c| c != b'\n').copied().collect();
        if let Ok(line) = String::from_utf8(shebang_line) {
            let mut parts: Vec<String> = line[2..].split_whitespace().map(|s| s.to_string()).collect();
            if !parts.is_empty() {
                let prefix = env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
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
/// number `\x7fELF`. Used to distinguish native binaries from scripts.
fn is_elf(filepath: &str) -> bool {
    use std::io::Read;
    let mut file = match fs::File::open(filepath) { Ok(f) => f, Err(_) => return false };
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer).is_ok() && buffer == [0x7f, b'E', b'L', b'F']
}

/// Copies an ELF binary from the SDCARD to `tmp_dir` and returns the cached path.
fn copy_elf_to_tmp(filepath: &str, tmp_dir: &str) -> Option<String> {
    let path = Path::new(filepath);
    let metadata = fs::metadata(path).ok()?;
    let absolute_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    fs::create_dir_all(tmp_dir).unwrap_or_default();
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
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
    let new_path_str = format!("{}/tnode_exec_{}_{}", tmp_dir, hash_val, filename_only);
    let new_path = Path::new(&new_path_str);
    if !new_path.exists() {
        if fs::copy(filepath, &new_path_str).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(mut perms) = fs::metadata(&new_path_str).map(|m| m.permissions()) {
                perms.set_mode(0o700);
                let _ = fs::set_permissions(&new_path_str, perms);
                return Some(new_path_str);
            }
        }
        return None;
    }
    Some(new_path_str)
}
