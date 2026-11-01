//! Available memory for choosing bounded part concurrency.

/// Bytes currently available to this process, when the operating system reports them.
/// On Linux, use the smaller of MemAvailable and remaining cgroup v2 headroom,
/// including ancestor limits (for example a systemd scope's MemoryMax).
pub fn available_memory() -> Option<u64> {
    platform_available_memory()
}

#[cfg(target_os = "linux")]
fn platform_available_memory() -> Option<u64> {
    linux_available_memory(
        std::path::Path::new("/proc/meminfo"),
        std::path::Path::new("/proc/self/cgroup"),
        std::path::Path::new("/sys/fs/cgroup"),
    )
}

#[cfg(target_os = "linux")]
fn linux_available_memory(
    meminfo: &std::path::Path,
    groups: &std::path::Path,
    root: &std::path::Path,
) -> Option<u64> {
    let host = std::fs::read_to_string(meminfo).ok().and_then(|text| {
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
    if let Ok(groups) = std::fs::read_to_string(groups) {
        if let Some(group) = groups.lines().find_map(|line| line.strip_prefix("0::")) {
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn platform_available_memory() -> Option<u64> {
    let mut system = sysinfo::System::new();
    system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
    let available = system.available_memory();
    let available = if available == 0 {
        system.free_memory()
    } else {
        available
    };
    (available > 0).then_some(available)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_available_memory() -> Option<u64> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_prefers_cgroup_headroom_including_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let meminfo = dir.path().join("meminfo");
        let groups = dir.path().join("cgroup");
        let root = dir.path().join("groups");
        let child = root.join("scope/worker");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(&meminfo, "MemAvailable: 1000 kB\n").unwrap();
        std::fs::write(&groups, "0::/scope/worker\n").unwrap();
        std::fs::write(child.join("memory.max"), "900000").unwrap();
        std::fs::write(child.join("memory.current"), "100000").unwrap();
        let read = || super::linux_available_memory(&meminfo, &groups, &root);
        assert_eq!(read(), Some(800000));
        std::fs::write(root.join("scope/memory.max"), "500000").unwrap();
        std::fs::write(root.join("scope/memory.current"), "200000").unwrap();
        assert_eq!(read(), Some(300000));
        std::fs::write(&meminfo, "MemAvailable: 100 kB\n").unwrap();
        assert_eq!(read(), Some(102400));
        std::fs::write(root.join("scope/memory.max"), "max").unwrap();
        assert_eq!(read(), Some(102400));
        std::fs::write(child.join("memory.current"), "900001").unwrap();
        assert_eq!(read(), Some(0));
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn available_memory_is_plausible() {
        let bytes = super::available_memory().unwrap();
        assert!(bytes > 0 && bytes < 1_000_000_000_000_000);
    }
}
