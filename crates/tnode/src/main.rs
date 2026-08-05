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

macro_rules! log_err {
    ($($arg:tt)*) => {
        eprintln!("\x1b[31m[tnode:ERROR]\x1b[0m {}", format_args!($($arg)*));
    }
}

macro_rules! log_warn {
    ($($arg:tt)*) => {
        eprintln!("\x1b[33m[tnode:WARN]\x1b[0m {}", format_args!($($arg)*));
    }
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        println!("\x1b[36m[tnode:INFO]\x1b[0m {}", format_args!($($arg)*));
    }
}

/// Top-level configuration structure deserialized from `bridge.toml`.
///
/// `bridge.toml` is read from the current working directory only (no global
/// fallback). All fields default to empty/disabled so a missing or partial
/// file never causes a panic.
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
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            tmp: None,
            lib: None,
            log: None,
            debug_level: None,
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
    tnode [TNODES_FLAGS] <COMMAND> [ARGS...]

DESCRIPTION:
    TNode is a smart wrapper that injects `libthook.so` via LD_PRELOAD into the 
    Node.js runtime environment on Android/Termux. It intercepts libc calls to 
    bypass restriction limitations (e.g., SDCARD noexec, linker namespaces).

TNODE FLAGS:
    --tnode-help       Print this help message and exit.
    --tnode-version    Print version information and exit.
    --tnode-update     Check and install the latest TNode release automatically.
    --tnode-changelog  Display the version changelog panel.
    --tnode-lib <path> Use a custom `libthook.so` path instead of auto-discovery.
    --tnode-tmp <path> Set a custom temporary directory for ELF caching bypass.
    --tnode-log <path> Output debug logs to the specified path.
    --tnode-debug <l>  Enable Syscall tracing logs (off, error, warn, info, trace).
    --tnode-clean      Garbage Collector: Cleans up cached ELFs > 24h old.
    --tnode-init       Initialize a default bridge.toml in the current directory.
    --tnode-bridge-off Disable bridge.toml config for this run.
    --tnode-bridge-on  Force enable bridge.toml config for this run.
    --tnode-env        Dump the active configuration and system environment variables.
    --tnode-env-export Print clean shell export statements (eval $(tnode --tnode-env-export)).
    --tnode-doctor     Run environment and system diagnostics check.

EXAMPLES:
    tnode npm install canvas
    tnode --tnode-update
    tnode --tnode-changelog
    tnode --tnode-debug trace --tnode-log ./debug.log npx prisma db push
    tnode --tnode-clean
    eval "$(tnode --tnode-env-export)"
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
                    if found {
                        break;
                    }
                    in_target = true;
                    found = true;
                    println!("\x1b[1mv{}\x1b[0m  \x1b[2m{}\x1b[0m", ver, date);
                    println!("\x1b[2m────────────────────────────────────────────────────\x1b[0m");
                } else if show_all {
                    if found {
                        println!();
                    }
                    found = true;
                    in_target = true;
                    println!("\x1b[1mv{}\x1b[0m  \x1b[2m{}\x1b[0m", ver, date);
                    println!("\x1b[2m────────────────────────────────────────────────────\x1b[0m");
                } else {
                    if ver == raw_target {
                        if found {
                            break;
                        }
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

        if !in_target {
            continue;
        }

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
                "Added" => println!("  \x1b[32m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                "Changed" => println!("  \x1b[34m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                "Removed" => println!("  \x1b[31m●\x1b[0m  \x1b[37m{}\x1b[0m", entry),
                _ => println!("  \x1b[37m{}\x1b[0m", entry),
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
        "x86_64" => Ok("x86_64".to_string()),
        other => Err(format!("Unsupported architecture: {}", other)),
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
        "macos" | "darwin" => Err("macOS is not supported because LD_PRELOAD is restricted by macOS SIP.".to_string()),
        other => Err(format!("Unsupported platform: {}", other)),
    }
}

/// Fetches the latest release tag_name from GitHub API via curl.
fn fetch_latest_release_tag() -> Option<String> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "--retry", "3",
            "https://api.github.com/repos/Irithell/tnode/releases/latest",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("\"tag_name\"") {
            if let Some(start) = line.find(':') {
                let rest = line[start + 1..].trim();
                let tag = rest.trim_matches(|c| c == '"' || c == ',' || c == ' ');
                if !tag.is_empty() {
                    return Some(tag.to_string());
                }
            }
        }
    }
    None
}

/// Performs a self-update of tnode and libthook.so using atomic rename.
fn handle_update() {
    println!("\x1b[36m[tnode]\x1b[0m Checking for updates...");

    let latest_tag = match fetch_latest_release_tag() {
        Some(tag) => tag,
        None => {
            log_err!("Failed to resolve latest version from GitHub API.");
            exit(1);
        }
    };

    let latest_ver = latest_tag.trim_start_matches('v');
    if latest_ver == VERSION {
        println!("\x1b[32m[tnode]\x1b[0m TNode is already up to date (v{}).", VERSION);
        exit(0);
    }

    println!("\x1b[36m[tnode]\x1b[0m Updating TNode v{} -> {}...", VERSION, latest_tag);

    let current_bin = match env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            log_err!("Failed to resolve current binary path: {}", e);
            exit(1);
        }
    };

    let bin_dir = current_bin.parent().unwrap_or_else(|| Path::new("/data/data/com.termux/files/usr/bin"));
    let lib_dir = bin_dir.parent().map(|p| p.join("lib")).unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr/lib"));
    let target_lib = lib_dir.join("libthook.so");

    let arch = match detect_arch() {
        Ok(a) => a,
        Err(e) => {
            log_err!("{}", e);
            exit(1);
        }
    };
    let platform = match detect_platform() {
        Ok(p) => p,
        Err(e) => {
            log_err!("{}", e);
            exit(1);
        }
    };

    let bin_asset = format!("tnode-{}-{}", arch, platform);
    let lib_asset = format!("libthook-{}-{}.so", arch, platform);
    let base_url = format!("https://github.com/Irithell/tnode/releases/download/{}/", latest_tag);

    let tmp_dir = env::var("THOOK_TMPDIR").unwrap_or_else(|_| env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string()));
    let update_tmp = PathBuf::from(&tmp_dir).join("tnode_update_download");
    let _ = fs::create_dir_all(&update_tmp);

    let tmp_bin = update_tmp.join("tnode");
    let tmp_lib = update_tmp.join("libthook.so");

    println!("\x1b[36m[tnode]\x1b[0m Downloading {}...", bin_asset);
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--progress-bar", "-o", tmp_bin.to_str().unwrap(), &format!("{}{}", base_url, bin_asset)])
        .status();
    if status.map(|s| !s.success()).unwrap_or(true) {
        log_err!("Failed to download binary asset: {}", bin_asset);
        exit(1);
    }

    println!("\x1b[36m[tnode]\x1b[0m Downloading {}...", lib_asset);
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--progress-bar", "-o", tmp_lib.to_str().unwrap(), &format!("{}{}", base_url, lib_asset)])
        .status();
    if status.map(|s| !s.success()).unwrap_or(true) {
        log_err!("Failed to download library asset: {}", lib_asset);
        exit(1);
    }

    // Atomic replacement helper
    let replace_file = |src: &Path, dst: &Path| -> Result<(), String> {
        let parent = dst.parent().ok_or_else(|| "Invalid destination directory".to_string())?;
        let _ = fs::create_dir_all(parent);
        let temp_dst = parent.join(format!(".tmp_replace_{}", dst.file_name().unwrap().to_string_lossy()));
        if let Err(e) = fs::copy(src, &temp_dst) {
            return Err(format!("Failed to write to destination directory: {}", e));
        }
        if let Err(e) = fs::rename(&temp_dst, dst) {
            let _ = fs::remove_file(&temp_dst);
            return Err(format!("Failed to replace {}: {}", dst.display(), e));
        }
        Ok(())
    };

    println!("\x1b[36m[tnode]\x1b[0m Installing update...");
    if let Err(e) = replace_file(&tmp_bin, &current_bin) {
        log_err!("{}", e);
        exit(1);
    }

    use std::os::unix::fs::PermissionsExt;
    if let Ok(mut perms) = fs::metadata(&current_bin).map(|m| m.permissions()) {
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&current_bin, perms);
    }

    if let Err(e) = replace_file(&tmp_lib, &target_lib) {
        log_err!("{}", e);
        exit(1);
    }

    let _ = fs::remove_dir_all(&update_tmp);

    println!("\x1b[32m[tnode]\x1b[0m Successfully updated to {}!\n", latest_tag);
    print_changelog(Some(&latest_ver));
}

/// Spawns a background thread to check for updates if 24h have passed since last check.
fn check_updates_background() {
    let cache_dir = get_cache_dir();
    let last_check_file = cache_dir.join("last_check");
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return,
    };

    let last_check: u64 = fs::read_to_string(&last_check_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

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

/// Prints update notification on stderr if a newer version tag was found in cache.
fn print_pending_update_notice() {
    let cache_dir = get_cache_dir();
    let latest_file = cache_dir.join("latest_version");
    if let Ok(tag) = fs::read_to_string(&latest_file) {
        let tag = tag.trim();
        let ver = tag.trim_start_matches('v');
        if !ver.is_empty() && ver != VERSION {
            eprintln!("\n\x1b[36m[tnode]\x1b[0m update available: v{} -> {}", VERSION, tag);
            eprintln!("\x1b[36m[tnode]\x1b[0m run: tnode --tnode-update\n");
        }
    }
}

/// Runs the `--tnode-doctor` diagnostic suite.
fn handle_doctor(config_path: Option<&PathBuf>, _config: &BridgeConfig, lib_path: Option<&PathBuf>, resolved_tmp: &str) {
    println!("\x1b[35m=== TNode System & Environment Doctor ===\x1b[0m\n");
    let mut errors = 0;
    let mut warnings = 0;

    // Check libthook.so
    print!("Checking libthook.so: ");
    if let Some(path) = lib_path {
        if path.exists() {
            println!("\x1b[32m[OK]\x1b[0m Found at {}", path.display());
        } else {
            println!("\x1b[31m[ERROR]\x1b[0m Path resolved to {} but file does not exist.", path.display());
            errors += 1;
        }
    } else {
        println!("\x1b[31m[ERROR]\x1b[0m Could not locate libthook.so in system.");
        errors += 1;
    }

    // Check TMPDIR access
    print!("Checking Temporary Directory ({}): ", resolved_tmp);
    let tmp_path = Path::new(resolved_tmp);
    if tmp_path.exists() {
        let test_file = tmp_path.join(".tnode_doctor_test");
        if fs::write(&test_file, "test").is_ok() {
            let _ = fs::remove_file(test_file);
            println!("\x1b[32m[OK]\x1b[0m Directory exists and is writable.");
        } else {
            println!("\x1b[31m[ERROR]\x1b[0m Directory exists but is NOT writable.");
            errors += 1;
        }
    } else {
        println!("\x1b[33m[WARN]\x1b[0m Directory does not exist. Attempting creation: ");
        if fs::create_dir_all(tmp_path).is_ok() {
            println!("\x1b[32m[OK]\x1b[0m Created successfully.");
        } else {
            println!("\x1b[31m[ERROR]\x1b[0m Failed to create directory.");
            errors += 1;
        }
    }

    // Check bridge.toml
    print!("Checking local bridge.toml: ");
    if let Some(p) = config_path {
        if p.exists() {
            println!("\x1b[32m[OK]\x1b[0m Found at {}", p.display());
        } else {
            println!("\x1b[33m[INFO]\x1b[0m File not present at {}", p.display());
        }
    } else {
        println!("\x1b[33m[INFO]\x1b[0m No bridge.toml in current directory.");
    }

    // Check essential binaries in PATH
    println!("\n\x1b[36mChecking essential tools in PATH:\x1b[0m");
    let tools = ["node", "npm", "clang", "python", "bun", "git"];
    for tool in &tools {
        print!("  tool {:<8}: ", tool);
        let status = Command::new("which").arg(tool).output();
        if let Ok(out) = status {
            if out.status.success() {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!("\x1b[32m[FOUND]\x1b[0m at {}", p);
            } else {
                println!("\x1b[33m[NOT FOUND]\x1b[0m");
                warnings += 1;
            }
        } else {
            println!("\x1b[33m[CHECK FAILED]\x1b[0m");
        }
    }

    // Check current working directory filesystem type
    println!("\n\x1b[36mChecking Current Working Directory:\x1b[0m");
    if let Ok(cwd) = env::current_dir() {
        let cwd_str = cwd.to_string_lossy();
        print!("  CWD: {} ", cwd_str);
        if cwd_str.contains("/storage/") || cwd_str.contains("/sdcard/") {
            println!("\x1b[33m[SDCARD/FUSE Mount Detected]\x1b[0m");
            println!("  -> libthook.so interception active for this session.");
        } else {
            println!("\x1b[32m[Internal Termux Filesystem]\x1b[0m");
        }
    }

    println!("\nDiagnostics complete: {} error(s), {} warning(s).", errors, warnings);
    if errors > 0 {
        exit(1);
    }
}

/// Returns the absolute form of `path`, joining to CWD if relative.
fn absolute_path(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        match env::current_dir() {
            Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
            Err(_) => path.to_string(),
        }
    }
}

/// Searches for `libthook.so` in system paths.
fn find_thook_lib() -> Option<PathBuf> {
    if let Ok(p) = env::var("THOOK_LIB_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let lib_dir = parent.join("../lib/libthook.so");
            if lib_dir.exists() {
                return Some(lib_dir);
            }
            let same_dir = parent.join("libthook.so");
            if same_dir.exists() {
                return Some(same_dir);
            }
            let cargo_debug = parent.join("../../target/debug/libthook.so");
            if cargo_debug.exists() {
                return Some(cargo_debug);
            }
            let cargo_release = parent.join("../../target/release/libthook.so");
            if cargo_release.exists() {
                return Some(cargo_release);
            }
        }
    }
    None
}

/// Removes cached ELF and shared library files older than 24 hours from `tmp_dir`.
fn handle_clean(tmp_dir: &str) {
    let path = Path::new(tmp_dir);
    if !path.exists() {
        log_info!("TMPDIR directory '{}' does not exist. Nothing to clean.", tmp_dir);
        return;
    }

    let read_dir = match fs::read_dir(path) {
        Ok(d) => d,
        Err(e) => {
            log_err!("Failed to read directory '{}': {}", tmp_dir, e);
            return;
        }
    };

    let now = std::time::SystemTime::now();
    let ttl = std::time::Duration::from_secs(86400); // 24 hours
    let mut cleaned_count = 0;
    let mut skipped_count = 0;

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("tnode_exec_") || name.starts_with("thook_dl_") || name.starts_with("thook_exec_") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > ttl {
                            if fs::remove_file(entry.path()).is_ok() {
                                cleaned_count += 1;
                                log_info!("Cleaned expired cache file: {}", name);
                            }
                        } else {
                            skipped_count += 1;
                        }
                    }
                }
            }
        }
    }

    log_info!("Garbage Collection complete: {} file(s) cleaned, {} file(s) active.", cleaned_count, skipped_count);
}

/// Writes a default `bridge.toml` to `config_path` if it does not already exist.
fn generate_default_config(config_path: &Path) {
    if config_path.exists() {
        return;
    }

    let default_tmp = env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string());
    
    let default_node_path = match Command::new("which").arg("node").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "/data/data/com.termux/files/usr/bin/node".to_string(),
    };

    let content = format!(r#"# bridge.toml — TNode project-level configuration
[core]
enabled = true
tmp = "{}"
lib = "/data/data/com.termux/files/usr/lib/libthook.so"
# log = "thook-{{timestamp}}.log"
debug_level = "off"

[aliases]
node = "{}"

[args]
# npm = ["--ignore-scripts"]

[env]
# PRISMA_QUERY_ENGINE_LIBRARY = "/path/to/libquery_engine.so.node"
"#, default_tmp, default_node_path);

    if fs::write(config_path, content).is_ok() {
        log_info!("Generated default bridge.toml at {}", config_path.display());
    }
}

/// Validates whether a temporary directory path is suitable for ELF caching and FIFO redirection.
///
/// Returns `false` if the path:
/// 1. Is located on an SDCARD / FUSE mount (`/storage/` or `/sdcard/`).
/// 2. Cannot be created or is not writable.
fn is_suitable_tmp_dir(path_str: &str) -> bool {
    if path_str.contains("/storage/") || path_str.contains("/sdcard/") {
        return false;
    }
    let p = Path::new(path_str);
    if fs::create_dir_all(p).is_err() {
        return false;
    }
    let test_file = p.join(".tnode_writable_test");
    if fs::write(&test_file, "test").is_ok() {
        let _ = fs::remove_file(test_file);
        true
    } else {
        false
    }
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
    let mut bridge_off = false;
    let mut bridge_on = false;
    let mut show_env = false;
    let mut show_env_export = false;
    let mut run_doctor = false;
    let mut should_init = false;
    let mut show_changelog = false;
    let mut changelog_target = None;
    let mut run_update = false;
    
    while let Some(arg) = iter.next() {
        if arg.starts_with("--tnode-") {
            match arg.as_str() {
                "--tnode-help" => {
                    print_help();
                    exit(0);
                }
                "--tnode-version" => {
                    print_version();
                    exit(0);
                }
                "--tnode-clean" => {
                    should_clean = true;
                }
                "--tnode-bridge-off" => bridge_off = true,
                "--tnode-bridge-on" => bridge_on = true,
                "--tnode-env" => show_env = true,
                "--tnode-env-export" => show_env_export = true,
                "--tnode-doctor" => run_doctor = true,
                "--tnode-init" => should_init = true,
                "--tnode-update" => run_update = true,
                "--tnode-changelog" => {
                    show_changelog = true;
                    if let Some(val) = iter.next() {
                        if !val.starts_with("--tnode-") && (val.starts_with('v') || val.chars().next().map_or(false, |c| c.is_ascii_digit())) {
                            changelog_target = Some(val);
                        } else {
                            target_command = Some(val);
                            target_args.extend(iter);
                            break;
                        }
                    }
                }
                "--tnode-debug" => {
                    let next_val = match iter.next() {
                        Some(val) if !val.starts_with("--tnode-") => val,
                        _ => {
                            log_err!("Missing or invalid argument for flag '{}'", arg);
                            exit(1);
                        }
                    };
                    inject_debug = Some(next_val);
                }
                "--tnode-lib" | "--tnode-log" | "--tnode-tmp" => {
                    let next_val = match iter.next() {
                        Some(val) if !val.starts_with("--tnode-") => val,
                        _ => {
                            log_err!("Missing or invalid argument for flag '{}'", arg);
                            exit(1);
                        }
                    };
                    match arg.as_str() {
                        "--tnode-lib" => custom_lib = Some(next_val),
                        "--tnode-log" => log_file = Some(absolute_path(&next_val)),
                        "--tnode-tmp" => custom_tmp = Some(absolute_path(&next_val)),
                        _ => unreachable!(),
                    }
                }
                _ => {
                    log_err!("Unknown flag '{}'", arg);
                    exit(1);
                }
            }
        } else {
            target_command = Some(arg);
            target_args.extend(iter);
            break;
        }
    }

    if show_changelog {
        print_changelog(changelog_target.as_deref());
        exit(0);
    }

    if run_update {
        handle_update();
        exit(0);
    }

    let local_config_path = PathBuf::from("./bridge.toml");

    if should_init {
        generate_default_config(&local_config_path);
        exit(0);
    }

    let mut config = BridgeConfig::default();
    let config_path_to_load = if local_config_path.exists() {
        Some(local_config_path.clone())
    } else {
        None
    };

    if let Some(ref path) = config_path_to_load {
        if let Ok(content) = fs::read_to_string(path) {
            match toml::from_str::<BridgeConfig>(&content) {
                Ok(parsed) => config = parsed,
                Err(e) => {
                    log_warn!("Failed to parse bridge.toml at {}: {}", path.display(), e);
                }
            }
        }
    }

    if bridge_off {
        config.core.enabled = Some(false);
    } else if bridge_on {
        config.core.enabled = Some(true);
    }

    let is_enabled = config.core.enabled.unwrap_or(true);

    let candidate_lib = custom_lib.map(PathBuf::from)
        .or_else(|| config.core.lib.as_ref().map(PathBuf::from));

    let resolved_lib_path = match candidate_lib {
        Some(path) => {
            if path.exists() && path.is_file() {
                Some(path)
            } else {
                log_warn!(
                    "Configured libthook.so path '{}' does not exist or is invalid. Falling back to auto-discovery...",
                    path.display()
                );
                find_thook_lib()
            }
        }
        None => find_thook_lib(),
    };

    let default_tmp = env::var("THOOK_TMPDIR")
        .unwrap_or_else(|_| env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string()));

    let candidate_tmp = custom_tmp
        .or(config.core.tmp.clone());

    let resolved_tmp = match candidate_tmp {
        Some(custom) => {
            if is_suitable_tmp_dir(&custom) {
                custom
            } else {
                log_warn!(
                    "Custom temporary directory '{}' is invalid, unwritable, or located on a noexec SDCARD partition. Falling back to default: {}",
                    custom, default_tmp
                );
                if is_suitable_tmp_dir(&default_tmp) {
                    default_tmp
                } else {
                    "/data/data/com.termux/files/usr/tmp".to_string()
                }
            }
        }
        None => {
            if is_suitable_tmp_dir(&default_tmp) {
                default_tmp
            } else {
                "/data/data/com.termux/files/usr/tmp".to_string()
            }
        }
    };

    if should_clean {
        handle_clean(&resolved_tmp);
        exit(0);
    }

    if run_doctor {
        handle_doctor(config_path_to_load.as_ref(), &config, resolved_lib_path.as_ref(), &resolved_tmp);
        exit(0);
    }

    if show_env {
        println!("=== TNode Bridge Active Configuration ===");
        println!("Core Enabled     : {}", is_enabled);
        println!("Resolved Lib     : {}", resolved_lib_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "NOT FOUND".to_string()));
        println!("Resolved TmpDir  : {}", resolved_tmp);
        println!("Debug Level      : {}", inject_debug.as_ref().or(config.core.debug_level.as_ref()).unwrap_or(&"off".to_string()));
        println!("Log File         : {}", log_file.as_ref().or(config.core.log.as_ref()).unwrap_or(&"stderr".to_string()));
        println!("\n[Aliases]");
        for (k, v) in &config.aliases {
            println!("  {:<12} -> {}", k, v);
        }
        println!("\n[Prepended Args]");
        for (k, v) in &config.args {
            println!("  {:<12} -> {:?}", k, v);
        }
        println!("\n[Injected Env]");
        for (k, v) in &config.env {
            println!("  {:<12} = {}", k, v);
        }
        exit(0);
    }

    let lib_path = match resolved_lib_path {
        Some(path) => path,
        None => {
            log_err!("Could not locate libthook.so. Make sure it is installed or set THOOK_LIB_PATH.");
            exit(1);
        }
    };

    if show_env_export {
        println!("export LD_PRELOAD=\"{}\"", lib_path.display());
        println!("export THOOK_TMPDIR=\"{}\"", resolved_tmp);
        if let Some(ref dbg) = inject_debug.or(config.core.debug_level) {
            println!("export THOOK_DEBUG_LEVEL=\"{}\"", dbg);
        }
        if let Some(ref log) = log_file.or(config.core.log) {
            println!("export THOOK_LOG_FILE=\"{}\"", log);
        }
        for (k, v) in &config.env {
            let val_str = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
            println!("export {}=\"{}\"", k, val_str);
        }
        exit(0);
    }

    let (raw_bin, user_args) = match target_command {
        Some(bin) => (bin, target_args),
        None => {
            print_help();
            exit(0);
        }
    };

    let bin = if is_enabled {
        config.aliases.get(&raw_bin).cloned().unwrap_or(raw_bin)
    } else {
        raw_bin
    };

    let mut final_args = Vec::new();
    if is_enabled {
        if let Some(extra_args) = config.args.get(&bin) {
            final_args.extend(extra_args.clone());
        }
    }
    final_args.extend(user_args);

    let debug_val = inject_debug.or(config.core.debug_level).unwrap_or_else(|| "off".to_string());
    let log_val = log_file.or(config.core.log);

    // Check updates in background thread (cached 1x/24h)
    check_updates_background();

    let attempt_execution = |bin: &str, args: &[String]| -> std::io::Result<std::process::ExitStatus> {
        let mut cmd = Command::new(bin);
        cmd.args(args);
        cmd.env("LD_PRELOAD", lib_path.to_string_lossy().to_string());
        
        for (k, v) in &config.env {
            let val_str = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
            cmd.env(k, val_str);
        }
        
        let b = bin.to_string();
        if b.contains("npm") || b.contains("npx") || b.contains("yarn") || b.contains("pnpm") || b.contains("node-gyp") {
            cmd.env("npm_config_android_ndk_path", "dummy");
        }

        cmd.env("THOOK_TMPDIR", &resolved_tmp);
        cmd.env("THOOK_DEBUG_LEVEL", &debug_val);
        
        if let Some(ref log_path) = log_val {
            let expanded_log = log_path
                .replace("{timestamp}", &std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs().to_string()).unwrap_or_default())
                .replace("{pid}", &std::process::id().to_string());
            cmd.env("THOOK_LOG_FILE", expanded_log);
        }
        
        cmd.status()
    };

    match attempt_execution(&bin, &final_args) {
        Ok(status) => {
            print_pending_update_notice();
            exit(status.code().unwrap_or(0));
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                log_warn!("EACCES executing '{}'. Attempting fallback execution...", bin);
                
                if let Some(shebang_args) = get_shebang(&bin) {
                    let mut fallback_args = shebang_args;
                    fallback_args.push(bin.clone());
                    fallback_args.extend(final_args.clone());
                    let interp = fallback_args.remove(0);
                    
                    if let Ok(st) = attempt_execution(&interp, &fallback_args) {
                        print_pending_update_notice();
                        exit(st.code().unwrap_or(0));
                    }
                } else if is_elf(&bin) {
                    if let Some(copied) = copy_elf_to_tmp(&bin, &resolved_tmp) {
                        if let Ok(st) = attempt_execution(&copied, &final_args) {
                            print_pending_update_notice();
                            exit(st.code().unwrap_or(0));
                        }
                    }
                }
            }
            
            log_err!("Failed to execute '{}': {}", bin, e);
            print_pending_update_notice();
            exit(1);
        }
    }
}

/// Reads the shebang line from `filepath` and returns the interpreter path and
/// any arguments embedded in the shebang line.
fn get_shebang(filepath: &str) -> Option<Vec<String>> {
    use std::io::Read;
    let mut file = match fs::File::open(filepath) {
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
                let prefix = env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
                if parts[0] == "/bin/sh" || parts[0] == "/bin/bash" {
                    parts[0] = format!("{}/bin/{}", prefix, if parts[0] == "/bin/sh" { "sh" } else { "bash" });
                } else if parts[0] == "/usr/bin/env" {
                    parts[0] = format!("{}/bin/env", prefix);
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
    let mut file = match fs::File::open(filepath) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buffer = [0u8; 4];
    if file.read_exact(&mut buffer).is_ok() {
        return buffer == [0x7f, b'E', b'L', b'F'];
    }
    false
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

