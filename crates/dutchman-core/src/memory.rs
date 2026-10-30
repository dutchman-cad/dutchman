//! Available memory for choosing bounded part concurrency.

/// Bytes currently available to this process, when the operating system reports them.
/// On Linux, use the smaller of MemAvailable and remaining cgroup v2 headroom,
/// including ancestor limits (for example a systemd scope's MemoryMax).
pub fn available_memory() -> Option<u64> {
    platform_available_memory()
}

#[cfg(target_os = "linux")]
fn platform_available_memory() -> Option<u64> {
    let host = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix("MemAvailable:")?;
                rest.split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)
            })
        });
    let mut available = host;
    if let Ok(groups) = std::fs::read_to_string("/proc/self/cgroup") {
        if let Some(group) = groups.lines().find_map(|line| line.strip_prefix("0::")) {
            let root = std::path::Path::new("/sys/fs/cgroup");
            let mut path = root.join(group.trim_start_matches('/'));
            while path.starts_with(root) {
                let read = |name| {
                    std::fs::read_to_string(path.join(name))
                        .ok()?
                        .trim()
                        .parse::<u64>()
                        .ok()
                };
                if let (Some(max), Some(current)) = (read("memory.max"), read("memory.current")) {
                    let remaining = max.saturating_sub(current);
                    available = Some(available.map_or(remaining, |host| host.min(remaining)));
                }
                if !path.pop() {
                    break;
                }
            }
        }
    }
    available
}

#[cfg(target_os = "macos")]
fn platform_available_memory() -> Option<u64> {
    let output = std::process::Command::new("vm_stat").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let page_size = text
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let pages = text
        .lines()
        .filter(|line| {
            ["Pages free:", "Pages inactive:", "Pages speculative:"]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .try_fold(0_u64, |sum, line| {
            let pages = line
                .split(':')
                .nth(1)?
                .trim()
                .trim_end_matches('.')
                .parse::<u64>()
                .ok()?;
            sum.checked_add(pages)
        })?;
    pages.checked_mul(page_size)
}

#[cfg(target_os = "windows")]
fn platform_available_memory() -> Option<u64> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_available_memory() -> Option<u64> {
    None
}
