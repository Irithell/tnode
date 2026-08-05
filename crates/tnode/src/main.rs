use std::env;
use std::process::{Command, exit};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::Deserialize;

// Read from Cargo.toml at compile time so --tnode-version always matches the crate version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    tnode --tnode-debug trace --tnode-log ./debug.log npx prisma db push
    tnode --tnode-clean
    eval "$(tnode --tnode-env-export)"
"#, VERSION);
}

/// Prints the version string to stdout.
fn print_version() {
    println!("tnode version {}", VERSION);
}

/// Runs the `--tnode-doctor` diagnostic suite.
///
/// Checks the following in order:
/// 1. `libthook.so` existence at the resolved path.
/// 2. `$TMPDIR` write access (creates it if absent).
/// 3. Presence of `bridge.toml` in the current directory.
/// 4. Availability of common tools (`node`, `npm`, `clang`, `python`, `bun`, `git`) in `$PATH`.
/// 5. Whether the current working directory is on an SDCARD FUSE filesystem.
///
/// Exits with 0 if no critical errors are found, 1 otherwise.
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
            println!("\x1b[31m[ERROR]\x1b[0m Configured/found path does not exist: {}", path.display());
            errors += 1;
        }
    } else {
        println!("\x1b[31m[ERROR]\x1b[0m libthook.so not found! Build or place in /data/data/com.termux/files/usr/lib/libthook.so");
        errors += 1;
    }

    // Check TMPDIR access
    print!("Checking Temporary Directory ({}): ", resolved_tmp);
    let tmp_path = Path::new(resolved_tmp);
    if tmp_path.exists() {
        let test_file = tmp_path.join(".tnode_doctor_test");
        match fs::write(&test_file, "doctor_test") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
                println!("\x1b[32m[OK]\x1b[0m Directory exists and is writable.");
            }
            Err(e) => {
                println!("\x1b[31m[ERROR]\x1b[0m Directory exists but is NOT writable: {}", e);
                errors += 1;
            }
        }
    } else {
        match fs::create_dir_all(tmp_path) {
            Ok(_) => {
                println!("\x1b[32m[OK]\x1b[0m Created successfully.");
            }
            Err(e) => {
                println!("\x1b[31m[ERROR]\x1b[0m Does not exist and failed to create: {}", e);
                errors += 1;
            }
        }
    }

    // Check bridge.toml
    print!("Checking local bridge.toml: ");
    if let Some(p) = config_path {
        if p.exists() {
            println!("\x1b[32m[OK]\x1b[0m Found at {}", p.display());
        } else {
            println!("\x1b[33m[INFO]\x1b[0m No bridge.toml in current directory (using defaults).");
        }
    } else {
        println!("\x1b[33m[INFO]\x1b[0m No bridge.toml in current directory.");
    }

    // Check essential binaries in PATH
    println!("\n\x1b[36mChecking essential tools in PATH:\x1b[0m");
    let tools = ["node", "npm", "clang", "python", "bun", "git"];
    for tool in &tools {
        match Command::new("which").arg(tool).output() {
            Ok(out) if out.status.success() => {
                let loc = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!("  - {:<8} : \x1b[32mFOUND\x1b[0m ({})", tool, loc);
            }
            _ => {
                println!("  - {:<8} : \x1b[33mNOT FOUND\x1b[0m", tool);
                if *tool == "node" {
                    warnings += 1;
                }
            }
        }
    }

    // Check current working directory filesystem type
    println!("\n\x1b[36mChecking Current Working Directory:\x1b[0m");
    if let Ok(cwd) = env::current_dir() {
        let cwd_str = cwd.to_string_lossy();
        print!("  Location: {} ", cwd_str);
        if cwd_str.starts_with("/sdcard") || cwd_str.starts_with("/storage") {
            println!("\x1b[33m(SDCARD FUSE filesystem - POSIX Shim active)\x1b[0m");
        } else {
            println!("\x1b[32m(Internal storage)\x1b[0m");
        }
    }

    println!("\n\x1b[35m=== Doctor Summary ===\x1b[0m");
    if errors == 0 {
        if warnings > 0 {
            println!("\x1b[33mStatus: Passed with {} warning(s).\x1b[0m", warnings);
        } else {
            println!("\x1b[32mStatus: All critical checks passed! TNode is ready to use.\x1b[0m");
        }
        exit(0);
    } else {
        println!("\x1b[31mStatus: Found {} critical error(s). Please review above issues.\x1b[0m", errors);
        exit(1);
    }
}

/// Returns the absolute form of `path`, joining to CWD if relative.
/// Falls back to returning the original string if `current_dir()` fails.
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

/// Searches for `libthook.so` in the following locations, in priority order:
///
/// 1. `$THOOK_LIB_PATH` environment variable (explicit override).
/// 2. `<exe_dir>/../lib/libthook.so` (global install layout: `bin/tnode`, `lib/libthook.so`).
/// 3. `<exe_dir>/libthook.so` (same-directory layout).
/// 4. `<exe_dir>/../../target/debug/libthook.so` (Cargo debug build).
/// 5. `<exe_dir>/../../target/release/libthook.so` (Cargo release build).
///
/// Returns `None` if none of the candidates exist.
fn find_thook_lib() -> Option<PathBuf> {
    if let Ok(p) = env::var("THOOK_LIB_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(exe_path) = env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            if let Some(parent_dir) = dir.parent() {
                let system_candidate = parent_dir.join("lib").join("libthook.so");
                if system_candidate.exists() { return Some(system_candidate); }
            }
            let candidate = dir.join("libthook.so");
            if candidate.exists() { return Some(candidate); }
            let lib_name = if cfg!(target_os = "macos") { "libthook.dylib" } else { "libthook.so" };
            let debug_candidate = dir.join("../../target/debug").join(lib_name);
            if debug_candidate.exists() { return Some(debug_candidate); }
            let release_candidate = dir.join("../../target/release").join(lib_name);
            if release_candidate.exists() { return Some(release_candidate); }
        }
    }
    None
}

/// Removes cached ELF and shared library files older than 24 hours from `tmp_dir`.
///
/// Only files whose names start with `tnode_exec_`, `thook_dl_`, or `thook_exec_`
/// are considered. Files younger than the TTL are skipped and reported as active.
fn handle_clean(tmp_dir: &str) {
    let path = Path::new(tmp_dir);
    if !path.exists() {
        log_info!("TMP directory '{}' does not exist. Nothing to clean.", tmp_dir);
        return;
    }
    let mut deleted = 0;
    let mut skipped = 0;
    let now = std::time::SystemTime::now();
    let ttl = std::time::Duration::from_secs(24 * 60 * 60);
    
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with("tnode_exec_") || filename.starts_with("thook_dl_") || filename.starts_with("thook_exec_") {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = now.duration_since(modified) {
                            if age > ttl {
                                if fs::remove_file(entry.path()).is_ok() {
                                    log_info!("Deleted cached file (age: >24h): {}", filename);
                                    deleted += 1;
                                }
                            } else {
                                skipped += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    if deleted > 0 {
        log_info!("Cleanup complete. {} cached files removed ({} still active).", deleted, skipped);
    } else if skipped > 0 {
        log_info!("No old cached files found ({} files are younger than 24h).", skipped);
    } else {
        log_info!("No cached files found to clean in '{}'.", tmp_dir);
    }
}

/// Writes a default `bridge.toml` to `config_path` if it does not already exist.
///
/// The template is populated with:
/// - `$TMPDIR` from the environment (falls back to the Termux default).
/// - The path of `node` as resolved by `which`.
/// - A hardcoded default `libthook.so` path.
///
/// The function is a no-op if `config_path` already exists so it can be called
/// unconditionally on startup without overwriting user edits.
fn generate_default_config(config_path: &Path) {
    if config_path.exists() {
        return;
    }
    let default_tmp = env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string());
    let node_path = match Command::new("which").arg("node").output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        _ => "/data/data/com.termux/files/usr/bin/node".to_string(),
    };
    let default_lib = "/data/data/com.termux/files/usr/lib/libthook.so";

    let template = format!(r#"# TNode Bridge Configuration File
# Auto-generated by tnode on first run.

[core]
# Master switch for the Bridge configurations.
# Set to false to disable aliases and args injection for this configuration scope.
enabled = true

# Global configurations for TNode operations.
tmp = "{}"
lib = "{}"

# Optional: Define a file to dump debug logs. 
# You can use {{timestamp}} and {{pid}} macros to separate logs per run.
# log = "thook-{{timestamp}}.log"

# Debug levels: "off", "error", "warn", "info", "trace"
# "error": Only critical failures
# "warn": Informational warnings and non-standard behavior
# "info": Basic lifecycle and main interceptions
# "trace": Massive mode (state of variables, all resolved paths and deep syscall logs)
debug_level = "off"

[aliases]
# Short aliases mapping to absolute executable paths.
# Example:
# python3 = "/data/data/com.termux/files/usr/bin/python3"
node = "{}"

[args]
# Arguments automatically injected BEFORE user arguments for specific aliases/commands.
# Example:
# npm = ["--ignore-scripts", "--verbose"]

[env]
# Environment variables automatically injected into the process.
# This works exactly like a .env file but is processed natively by tnode.
# Example:
# PRISMA_QUERY_ENGINE_LIBRARY = "/path/to/libquery_engine.so.node"
"#, default_tmp, default_lib, node_path);

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).unwrap_or_default();
    }
    if fs::write(config_path, template).is_ok() {
        log_info!("Generated default bridge.toml at {}", config_path.display());
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

    if let Some(path) = config_path_to_load.as_ref() {
        if let Ok(contents) = fs::read_to_string(path) {
            match toml::from_str::<BridgeConfig>(&contents) {
                Ok(parsed) => {
                    config = parsed;
                }
                Err(e) => {
                    if !show_env_export {
                        log_warn!("Invalid bridge.toml syntax in {}, ignoring config: {}", path.display(), e);
                    }
                }
            }
        }
    }

    let is_enabled = if bridge_off {
        false
    } else if bridge_on {
        true
    } else {
        config.core.enabled.unwrap_or(true)
    };

    let resolved_tmp = custom_tmp.clone().or_else(|| config.core.tmp.clone()).unwrap_or_else(|| {
        env::var("TMPDIR").unwrap_or_else(|_| "/data/data/com.termux/files/usr/tmp".to_string())
    });

    let lib_path_opt = custom_lib.clone().or_else(|| config.core.lib.clone()).map(PathBuf::from).or_else(find_thook_lib);

    if run_doctor {
        handle_doctor(config_path_to_load.as_ref(), &config, lib_path_opt.as_ref(), &resolved_tmp);
        exit(0);
    }

    let debug_val = inject_debug.clone().or_else(|| config.core.debug_level.clone()).unwrap_or_else(|| "off".to_string());

    if show_env_export {
        let final_lib_path = lib_path_opt.unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr/lib/libthook.so"));
        println!("export LD_PRELOAD=\"{}\"", final_lib_path.display());
        println!("export THOOK_TMPDIR=\"{}\"", resolved_tmp);
        if debug_val != "off" {
            println!("export THOOK_DEBUG_LEVEL=\"{}\"", debug_val);
        }
        if is_enabled {
            for (k, v) in &config.env {
                let val_str = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
                let escaped_val = val_str.replace('\\', "\\\\").replace('"', "\\\"");
                println!("export {}=\"{}\"", k, escaped_val);
            }
        }
        exit(0);
    }

    if show_env {
        println!("\x1b[35m=== TNode Active Environment ===\x1b[0m");
        println!("\x1b[36mLoaded Config File:\x1b[0m {:?}", config_path_to_load);
        println!("\x1b[36mBridge Features Enabled:\x1b[0m {}", is_enabled);
        println!("\x1b[36mCore Settings:\x1b[0m");
        println!("  - Tmp: {:?}", custom_tmp.as_ref().or(config.core.tmp.as_ref()));
        println!("  - Lib: {:?}", custom_lib.as_ref().or(config.core.lib.as_ref()));
        println!("  - Log: {:?}", log_file.as_ref().or(config.core.log.as_ref()));
        println!("  - Debug Level: {:?}", inject_debug.as_ref().or(config.core.debug_level.as_ref()));
        
        if is_enabled {
            println!("\x1b[36mAliases:\x1b[0m");
            for (k, v) in &config.aliases {
                println!("  - {} -> {}", k, v);
            }
            println!("\x1b[36mInjected Args:\x1b[0m");
            for (k, v) in &config.args {
                println!("  - {} -> {:?}", k, v);
            }
            println!("\x1b[36mInjected Env Vars:\x1b[0m");
            for (k, v) in &config.env {
                let val_str = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
                println!("  - {} = {:?}", k, val_str);
            }
        }

        println!("\n\x1b[35m=== System Environment Variables ===\x1b[0m");
        for (k, v) in env::vars() {
            println!("  - \x1b[36m{}\x1b[0m = {:?}", k, v);
        }
        exit(0);
    }

    if should_clean {
        handle_clean(&resolved_tmp);
        if target_command.is_none() { exit(0); }
    }

    let mut final_bin = match target_command {
        Some(cmd) => cmd,
        None => {
            log_err!("No target command specified. Try --tnode-help.");
            exit(1);
        },
    };

    if is_enabled {
        if let Some(alias_path) = config.aliases.get(&final_bin) {
            final_bin = alias_path.clone();
        }
        let bin_name = Path::new(&final_bin).file_name().unwrap_or_default().to_string_lossy().to_string();
        if let Some(injected_args) = config.args.get(&bin_name) {
            let mut new_args = injected_args.clone();
            new_args.extend(target_args);
            target_args = new_args;
        }
    }

    let lib_path = match custom_lib.or(config.core.lib) {
        Some(p) => {
            let path = PathBuf::from(p);
            if !path.exists() {
                log_err!("Custom lib path does not exist: {:?}", path);
                exit(1);
            }
            path
        }
        None => match find_thook_lib() {
            Some(p) => p,
            None => {
                log_err!("Could not find libthook.so automatically. Try passing --tnode-lib <path>.");
                exit(1);
            }
        }
    };

    let debug_val = inject_debug.or(config.core.debug_level).unwrap_or_else(|| "off".to_string());
    let log_val = log_file.or(config.core.log);

    let attempt_execution = |bin: &str, args: &[String]| -> std::io::Result<std::process::ExitStatus> {
        let mut cmd = Command::new(bin);
        cmd.args(args);
        cmd.env("LD_PRELOAD", lib_path.to_string_lossy().to_string());
        
        for (k, v) in &config.env {
            let val_str = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
            cmd.env(k, val_str);
        }
        
        let b = bin.to_string();
        // Node-Gyp requires ANDROID_NDK_PATH to be set to a non-empty value.
        // Using the npm_config_ prefix causes npm to emit an "Unknown env config" warning.
        // ANDROID_NDK_PATH is read directly by node-gyp's OS/JS evaluation layer.
        if b.contains("npm") || b.contains("npx") || b.contains("yarn") || b.contains("pnpm") || b.contains("node-gyp") {
            cmd.env("npm_config_android_ndk_path", "dummy");
        }
        
        if debug_val != "off" {
            cmd.env("THOOK_DEBUG_LEVEL", &debug_val);
        }
        if let Some(ref log) = log_val {
            let mut final_log = log.clone();
            if final_log.contains("{timestamp}") {
                if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    final_log = final_log.replace("{timestamp}", &duration.as_secs().to_string());
                }
            }
            if final_log.contains("{pid}") {
                final_log = final_log.replace("{pid}", &std::process::id().to_string());
            }
            cmd.env("THOOK_LOG_FILE", absolute_path(&final_log));
        }
        cmd.env("THOOK_TMPDIR", &resolved_tmp);
        
        cmd.status()
    };

    match attempt_execution(&final_bin, &target_args) {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) => {
            if e.raw_os_error() == Some(13) {
                if let Some(shebang_args) = get_shebang(&final_bin) {
                    let interpreter = &shebang_args[0];
                    let mut new_args = Vec::new();
                    for arg in shebang_args.iter().skip(1) { new_args.push(arg.clone()); }
                    new_args.push(final_bin.clone());
                    new_args.extend_from_slice(&target_args);
                    match attempt_execution(interpreter, &new_args) {
                        Ok(status) => exit(status.code().unwrap_or(1)),
                        Err(e2) => {
                            log_err!("Failed to execute shebang wrapper: {}", e2);
                            exit(1);
                        }
                    }
                } else if is_elf(&final_bin) {
                    if let Some(copied_path) = copy_elf_to_tmp(&final_bin, &resolved_tmp) {
                        match attempt_execution(&copied_path, &target_args) {
                            Ok(status) => exit(status.code().unwrap_or(1)),
                            Err(e2) => {
                                log_err!("Failed to execute copied ELF: {}", e2);
                                exit(1);
                            }
                        }
                    }
                }
            }
            log_err!("Failed to execute {}: {}", final_bin, e);
            exit(1);
        }
    }
}

/// Reads the shebang line from `filepath` and returns the interpreter path and
/// any arguments embedded in the shebang line.
///
/// Rewriting rules (same as in `libthook`):
/// - `/bin/sh` -> `$PREFIX/bin/sh`
/// - `/bin/bash` -> `$PREFIX/bin/bash`
/// - `/usr/bin/env` -> `$PREFIX/bin/env`
///
/// Returns `None` if the file cannot be opened, is too short, or has no shebang.
fn get_shebang(filepath: &str) -> Option<Vec<String>> {
    use std::io::Read;
    let mut file = match std::fs::File::open(filepath) {
        Ok(f) => f,
        Err(_) => return None,
    };
    let mut buffer = [0u8; 256];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);
    if bytes_read < 2 { return None; }
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

/// Copies an ELF binary from the SDCARD to `tmp_dir` and returns the cached path.
///
/// The filename includes a hash derived from the absolute source path, the
/// modification time, and the file size for cache invalidation. The copy is
/// made executable. Returns `None` on any I/O failure.
fn copy_elf_to_tmp(filepath: &str, tmp_dir: &str) -> Option<String> {
    let path = Path::new(filepath);
    let metadata = std::fs::metadata(path).ok()?;
    let absolute_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    std::fs::create_dir_all(tmp_dir).unwrap_or_default();
    
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
        if std::fs::copy(filepath, &new_path_str).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(mut perms) = std::fs::metadata(&new_path_str).map(|m| m.permissions()) {
                perms.set_mode(0o700);
                let _ = std::fs::set_permissions(&new_path_str, perms);
                return Some(new_path_str);
            }
        }
        return None;
    }
    Some(new_path_str)
}
