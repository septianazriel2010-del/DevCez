use serde::Serialize;
use sysinfo::System;

#[derive(Debug, Serialize)]
pub struct SystemSnapshot {
    pub app_version: String,
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub cpu_usage: f32,
    pub memory_used_gb: f64,
    pub memory_total_gb: f64,
    pub memory_usage: f32,
    pub logical_cores: usize,
}

#[tauri::command]
pub fn get_system_snapshot() -> Result<SystemSnapshot, String> {
    let mut system = System::new_all();
    system.refresh_all();

    let cpus = system.cpus();
    let cpu_usage = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32
    };

    // sysinfo reports memory in bytes. Convert to GiB for the dashboard.
    let memory_total_gb = system.total_memory() as f64 / 1_073_741_824.0;
    let memory_used_gb = system.used_memory() as f64 / 1_073_741_824.0;
    let memory_usage = if memory_total_gb > 0.0 {
        ((memory_used_gb / memory_total_gb) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    Ok(SystemSnapshot {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_name: System::name().unwrap_or_else(|| "Unknown OS".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "Unknown version".to_string()),
        host_name: System::host_name().unwrap_or_else(|| "Unknown device".to_string()),
        cpu_usage: cpu_usage.clamp(0.0, 100.0),
        memory_used_gb,
        memory_total_gb,
        memory_usage,
        logical_cores: cpus.len(),
    })
}
