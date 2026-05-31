use std::thread;

#[derive(Debug, Clone)]
pub struct HardwareProfile {
    pub total_ram_gb: f32,
    pub physical_cores: usize,
    pub logical_cores: usize,
}

impl HardwareProfile {
    pub fn detect() -> Self {
        let logical_cores = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let total_ram_gb = Self::detect_ram_gb();
        let physical_cores = Self::detect_physical_cores(logical_cores);

        tracing::info!("--- 🖥️  Hardware Detected ({}) ---", std::env::consts::OS);
        tracing::info!("  💾 RAM: {:.2} GB", total_ram_gb);
        tracing::info!("  ⚙️  CPU: {} Cores ({} Logical)", physical_cores, logical_cores);
        tracing::info!("----------------------------");

        HardwareProfile { total_ram_gb, physical_cores, logical_cores }
    }

    #[cfg(target_os = "linux")]
    fn detect_ram_gb() -> f32 {
        use std::fs;
        let content = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let kb = content.lines()
            .find(|l| l.starts_with("MemTotal:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(8 * 1024 * 1024);
        (kb as f32) / 1_048_576.0
    }

    #[cfg(target_os = "macos")]
    fn detect_ram_gb() -> f32 {
        use std::process::Command;
        let out = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(8 * 1024 * 1024 * 1024);
        (out as f32) / 1_073_741_824.0
    }

    #[cfg(target_os = "windows")]
    fn detect_ram_gb() -> f32 {
        use std::process::Command;
        let out = Command::new("powershell")
            .args(["-Command", "(Get-CimInstance Win32_PhysicalMemory | Measure-Object -Property Capacity -Sum).Sum"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(8 * 1024 * 1024 * 1024);
        (out as f32) / 1_073_741_824.0
    }

    #[cfg(target_os = "linux")]
    fn detect_physical_cores(logical: usize) -> usize {
        use std::fs;
        let content = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        content.lines()
            .filter(|l| l.starts_with("cpu cores"))
            .filter_map(|l| l.split(':').nth(1))
            .filter_map(|v| v.trim().parse::<usize>().ok())
            .next()
            .unwrap_or(logical / 2)
            .max(1)
    }

    #[cfg(target_os = "macos")]
    fn detect_physical_cores(logical: usize) -> usize {
        use std::process::Command;
        Command::new("sysctl")
            .args(["-n", "hw.physicalcpu"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(logical / 2)
            .max(1)
    }

    #[cfg(target_os = "windows")]
    fn detect_physical_cores(logical: usize) -> usize {
        use std::process::Command;
        Command::new("wmic")
            .args(["cpu", "get", "NumberOfCores"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.lines().nth(1).and_then(|v| v.trim().parse::<usize>().ok()))
            .unwrap_or(logical / 2)
            .max(1)
    }
}
