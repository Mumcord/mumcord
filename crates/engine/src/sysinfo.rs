//! System details for crash reports and diagnostics (#568): OS, kernel, desktop, CPU with its
//! instruction sets, graphics cards with driver versions and memory. Read from files and a few
//! `--version` style tools; nothing personal (no host name, user name or serial numbers).

#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::time::Duration;

/// One line per fact, e.g. `cpu: …`. Missing facts are left out.
pub fn collect() -> Vec<String> {
    let mut lines = Vec::new();
    let mut push = |key: &str, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
            lines.push(format!("{key}: {}", value.trim()));
        }
    };
    push("os", os_name());
    push("kernel", kernel());
    push("desktop", desktop());
    push("cpu", cpu_model());
    push("instruction sets", Some(instruction_sets().join(" ")));
    for (i, gpu) in gpus().into_iter().enumerate() {
        push(&format!("gpu {}", i + 1), Some(gpu));
    }
    push("opengl", opengl());
    push("memory", memory());
    lines
}

/// The x86 or ARM instruction set extensions this CPU has, as the program sees them.
pub fn instruction_sets() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut found: Vec<&'static str> = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        macro_rules! check {
            ($($feature:tt),*) => {
                $(if std::arch::is_x86_feature_detected!($feature) { found.push($feature); })*
            };
        }
        check!(
            "sse2",
            "sse3",
            "ssse3",
            "sse4.1",
            "sse4.2",
            "popcnt",
            "aes",
            "pclmulqdq",
            "avx",
            "f16c",
            "fma",
            "bmi1",
            "bmi2",
            "avx2",
            "avx512f",
            "avx512bw",
            "avx512vl",
            "sha"
        );
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! check {
            ($($feature:tt),*) => {
                $(if std::arch::is_aarch64_feature_detected!($feature) { found.push($feature); })*
            };
        }
        check!("neon", "aes", "sha2", "crc", "dotprod", "sve");
    }
    found
}

/// Runs a version tool briefly; `None` when it is missing, fails or takes too long.
#[cfg(target_os = "linux")]
fn tool(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    for _ in 0..30 {
        if let Some(status) = child.try_wait().ok()? {
            if !status.success() {
                return None;
            }
            let mut out = String::new();
            std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
            return Some(out);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    None
}

#[cfg(target_os = "linux")]
fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(target_os = "linux")]
fn os_name() -> Option<String> {
    let release = read("/etc/os-release").or_else(|| read("/usr/lib/os-release"))?;
    let field = |name: &str| {
        release.lines().find_map(|l| {
            l.strip_prefix(&format!("{name}="))
                .map(|v| v.trim_matches('"').to_owned())
        })
    };
    let pretty = field("PRETTY_NAME").or_else(|| field("NAME"))?;
    Some(match field("VERSION_ID") {
        Some(version) if !pretty.contains(&version) => format!("{pretty} ({version})"),
        _ => pretty,
    })
}

#[cfg(not(target_os = "linux"))]
fn os_name() -> Option<String> {
    Some(crate::diag::os_description())
}

#[cfg(target_os = "linux")]
fn kernel() -> Option<String> {
    read("/proc/sys/kernel/osrelease")
}

#[cfg(not(target_os = "linux"))]
fn kernel() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn desktop() -> Option<String> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let name = var("XDG_CURRENT_DESKTOP").unwrap_or_else(|| "unknown".to_owned());
    let session = var("XDG_SESSION_TYPE").unwrap_or_else(|| {
        if var("WAYLAND_DISPLAY").is_some() {
            "wayland".to_owned()
        } else if var("DISPLAY").is_some() {
            "x11".to_owned()
        } else {
            "unknown".to_owned()
        }
    });
    let lower = name.to_ascii_lowercase();
    let version = if lower.contains("gnome") {
        tool("gnome-shell", &["--version"])
    } else if lower.contains("kde") {
        tool("plasmashell", &["--version"])
    } else if lower.contains("xfce") {
        tool("xfce4-session", &["--version"]).and_then(|v| v.lines().next().map(str::to_owned))
    } else {
        None
    };
    let compositor = if session == "wayland" {
        var("WAYLAND_DISPLAY").map(|d| format!(", display {d}"))
    } else {
        None
    };
    Some(format!(
        "{name} on {session}{}{}",
        version
            .map(|v| format!(", {}", v.trim()))
            .unwrap_or_default(),
        compositor.unwrap_or_default()
    ))
}

#[cfg(not(target_os = "linux"))]
fn desktop() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn cpu_model() -> Option<String> {
    let info = read("/proc/cpuinfo")?;
    let model = info
        .lines()
        .find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')))
        .map(|(_, v)| v.trim().to_owned())?;
    let threads = info.lines().filter(|l| l.starts_with("processor")).count();
    Some(format!("{model}, {threads} threads"))
}

#[cfg(not(target_os = "linux"))]
fn cpu_model() -> Option<String> {
    std::thread::available_parallelism()
        .ok()
        .map(|n| format!("{} threads ({})", n, std::env::consts::ARCH))
}

/// Every DRM card: PCI ids, name from `lspci` when available, the kernel driver and its
/// version (the NVIDIA driver reports its own).
#[cfg(target_os = "linux")]
fn gpus() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("card") && !n.contains('-'))
        .collect();
    cards.sort();
    cards
        .iter()
        .filter_map(|card| {
            let device = format!("/sys/class/drm/{card}/device");
            let id = |f: &str| {
                read(&format!("{device}/{f}")).map(|v| v.trim().trim_start_matches("0x").to_owned())
            };
            let (vendor, product) = (id("vendor")?, id("device")?);
            let driver = std::fs::read_link(format!("{device}/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            let name =
                tool("lspci", &["-mm", "-d", &format!("{vendor}:{product}")]).and_then(|out| {
                    let line = out.lines().next()?.to_owned();
                    let fields: Vec<&str> =
                        line.split('"').filter(|f| !f.trim().is_empty()).collect();
                    Some(format!("{} {}", fields.get(2)?, fields.get(3)?))
                });
            let version = match driver.as_deref() {
                Some("nvidia") => read("/proc/driver/nvidia/version")
                    .and_then(|v| v.lines().next().map(str::to_owned)),
                Some(module) => read(&format!("/sys/module/{module}/version")),
                None => None,
            };
            Some(format!(
                "{} [{vendor}:{product}], driver {}{}",
                name.unwrap_or_else(|| "unknown".to_owned()),
                driver.unwrap_or_else(|| "none".to_owned()),
                version
                    .map(|v| format!(" {}", v.trim()))
                    .unwrap_or_default()
            ))
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn gpus() -> Vec<String> {
    Vec::new()
}

/// Renderer and version from `glxinfo -B` (Mesa or NVIDIA), when installed.
#[cfg(target_os = "linux")]
fn opengl() -> Option<String> {
    let out = tool("glxinfo", &["-B"])?;
    let field = |name: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_owned()))
    };
    Some(format!(
        "{}; {}",
        field("OpenGL renderer string:")?,
        field("OpenGL version string:")?
    ))
}

#[cfg(not(target_os = "linux"))]
fn opengl() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn memory() -> Option<String> {
    let info = read("/proc/meminfo")?;
    let kib = |name: &str| {
        info.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|r| r.split_whitespace().next()?.parse::<u64>().ok())
    };
    Some(format!(
        "{} MiB total, {} MiB available",
        kib("MemTotal:")? / 1024,
        kib("MemAvailable:")? / 1024
    ))
}

#[cfg(not(target_os = "linux"))]
fn memory() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_at_least_the_os_and_the_instruction_sets() {
        let lines = collect();
        assert!(lines.iter().any(|l| l.starts_with("os: ")), "{lines:?}");
        #[cfg(target_arch = "x86_64")]
        assert!(instruction_sets().contains(&"sse2"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_tool_is_none() {
        assert!(tool("mumcord-no-such-tool", &["--version"]).is_none());
    }
}
