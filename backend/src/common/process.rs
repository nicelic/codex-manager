use std::process::Command;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub id: u32,
    pub name: String,
}

pub fn is_process_running(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {}", name), "/NH"])
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            return text.to_lowercase().contains(&name.to_lowercase());
        }
    }
    false
}

pub fn find_process_id(name: &str) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {}", name), "/FO", "CSV", "/NH"])
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim_matches('"')).collect();
                if parts.len() >= 2 && parts[0].eq_ignore_ascii_case(name) {
                    if let Ok(pid) = parts[1].parse::<u32>() {
                        return Some(pid);
                    }
                }
            }
        }
    }
    None
}

pub fn kill_process_by_name(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("taskkill")
            .args(["/F", "/IM", name])
            .status();
        return status.map(|s| s.success()).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}
