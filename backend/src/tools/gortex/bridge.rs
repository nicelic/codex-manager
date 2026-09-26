use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::service::GortexService;
use crate::common::windows::{canonicalize_clean, clean_path_str, strip_windows_verbatim_prefix};

fn is_ambiguous_directory(p: &Path) -> bool {
    let s = p.to_string_lossy();
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "/" || trimmed == "." || trimmed == "\\" {
        return true;
    }
    if p.is_absolute() && p.parent().is_none() {
        return true;
    }
    false
}

fn is_host_program_directory(p: &Path) -> bool {
    let lower = p.to_string_lossy().to_ascii_lowercase();
    lower.contains(r"\programs\antigravity")
        || lower.contains("/programs/antigravity")
        || lower.contains(r"\microsoft vs code")
        || lower.contains("/microsoft vs code")
}

pub fn resolve_bridge_target_cwd(gortex_exe: &Path) -> Option<PathBuf> {
    // 1. Check explicit environment variables
    for key in &["ANTIGRAVITY_WORKSPACE", "WORKSPACE", "WORKSPACE_DIR", "PROJECT_DIR"] {
        if let Ok(val) = std::env::var(key) {
            let cleaned = clean_path_str(&val);
            if !cleaned.is_empty() {
                let p = PathBuf::from(cleaned);
                if p.is_dir() && !is_ambiguous_directory(&p) && !is_host_program_directory(&p) {
                    return canonicalize_clean(&p).ok().or_else(|| Some(strip_windows_verbatim_prefix(&p)));
                }
            }
        }
    }

    // 2. Check current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let clean_cwd = strip_windows_verbatim_prefix(&cwd);
        if clean_cwd.is_dir() && !is_ambiguous_directory(&clean_cwd) && !is_host_program_directory(&clean_cwd) {
            return canonicalize_clean(&clean_cwd).ok().or_else(|| Some(clean_cwd));
        }
    }

    // 3. Check tracked projects from daemon status
    let exe_str = gortex_exe.to_string_lossy().to_string();
    let tracked = GortexService::query_daemon_tracked_projects(&exe_str);
    for p_str in tracked {
        let cleaned = clean_path_str(&p_str);
        if !cleaned.is_empty() {
            let p = PathBuf::from(cleaned);
            if p.is_dir() && !is_ambiguous_directory(&p) && !is_host_program_directory(&p) {
                return canonicalize_clean(&p).ok().or_else(|| Some(strip_windows_verbatim_prefix(&p)));
            }
        }
    }

    // 4. Check locally registered tracked projects
    let local = GortexService::get_tracked_projects();
    for p_str in local {
        let cleaned = clean_path_str(&p_str);
        if !cleaned.is_empty() {
            let p = PathBuf::from(cleaned);
            if p.is_dir() && !is_ambiguous_directory(&p) && !is_host_program_directory(&p) {
                return canonicalize_clean(&p).ok().or_else(|| Some(strip_windows_verbatim_prefix(&p)));
            }
        }
    }

    // 5. Fallback to user home directory
    let home = strip_windows_verbatim_prefix(crate::common::windows::get_user_profile_dir());
    if home.is_dir() {
        return Some(home);
    }

    None
}

pub fn run_gortex_bridge(args: &[String]) -> ! {
    let mut gortex_exe = String::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--gortex" && i + 1 < args.len() {
            gortex_exe = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }

    if gortex_exe.is_empty() {
        if let Ok(val) = std::env::var("GORTEX_EXECUTABLE") {
            gortex_exe = val;
        }
    }

    if gortex_exe.is_empty() {
        let managed = GortexService::get_managed_executable_path();
        if let Some(m) = managed {
            gortex_exe = m.to_string_lossy().to_string();
        }
    }

    if gortex_exe.is_empty() {
        let direct = GortexService::get_executable_path();
        gortex_exe = direct.to_string_lossy().to_string();
    }

    let exe_path = PathBuf::from(&gortex_exe);
    if !exe_path.exists() {
        eprintln!("[gortex-bridge] 未找到有效的 Gortex 可执行文件: {}", gortex_exe);
        std::process::exit(1);
    }

    let target_cwd = resolve_bridge_target_cwd(&exe_path);

    let mut cmd = Command::new(&exe_path);
    cmd.arg("mcp");

    if let Some(ref cwd) = target_cwd {
        cmd.current_dir(cwd);
    }

    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Managed environment variables
    for (k, v) in GortexService::get_managed_env_vars(&exe_path) {
        cmd.env(k, v);
    }

    if let Some(ref cwd) = target_cwd {
        cmd.env("ANTIGRAVITY_WORKSPACE", clean_path_str(&cwd.to_string_lossy()));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    match cmd.status() {
        Ok(status) => {
            std::process::exit(status.code().unwrap_or(0));
        }
        Err(e) => {
            eprintln!("[gortex-bridge] Gortex 进程启动失败: {}", e);
            std::process::exit(1);
        }
    }
}
