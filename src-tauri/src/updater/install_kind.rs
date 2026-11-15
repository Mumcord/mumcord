//! How this copy of Mumcord was installed, which decides how it can be updated (FR-3.147).
//! The app never writes into system paths: packages are left to the package manager.

use std::path::Path;

use crate::commands::updates::InstallKind;

impl InstallKind {
    /// The bundle type in the update manifest (`{os}-{arch}-{installer}`), for the kinds the
    /// updater replaces by itself.
    fn installer(self) -> Option<&'static str> {
        match self {
            Self::AppImage => Some("appimage"),
            Self::WindowsInstaller => Some("nsis"),
            Self::MacApp => Some("app"),
            _ => None,
        }
    }

    /// The update manifest key for this install, `None` when it does not update itself.
    pub fn manifest_target(self) -> Option<String> {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        };
        let arch = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x86_64"
        };
        Some(format!("{os}-{arch}-{}", self.installer()?))
    }
}

/// What the Linux checks found; split out so the decision is testable.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
struct LinuxFacts {
    /// `$APPIMAGE` names the running AppImage.
    appimage: bool,
    /// The AppImage and its folder can be written by the user.
    appimage_writable: bool,
    arch: bool,
    dpkg_owns_us: bool,
    rpm: bool,
    /// The Mumcord APT source file exists.
    apt_repo: bool,
    /// The Mumcord DNF repo file exists.
    dnf_repo: bool,
    /// The Mumcord zypper repo file exists.
    zypper_repo: bool,
    /// `/etc/pacman.conf` has a `[mumcord]` section.
    pacman_repo: bool,
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn linux_kind(facts: &LinuxFacts) -> InstallKind {
    if facts.appimage {
        return if facts.appimage_writable {
            InstallKind::AppImage
        } else {
            InstallKind::AppImageReadOnly
        };
    }
    if facts.arch {
        if facts.pacman_repo {
            InstallKind::PacmanRepo
        } else {
            InstallKind::Aur
        }
    } else if facts.dpkg_owns_us {
        if facts.apt_repo {
            InstallKind::AptRepo
        } else {
            InstallKind::Deb
        }
    } else if facts.rpm {
        if facts.zypper_repo {
            InstallKind::ZypperRepo
        } else if facts.dnf_repo {
            InstallKind::DnfRepo
        } else {
            InstallKind::Rpm
        }
    } else {
        InstallKind::OtherLinux
    }
}

#[cfg(target_os = "linux")]
fn writable(path: &Path) -> bool {
    std::fs::OpenOptions::new().append(true).open(path).is_ok()
        && path.parent().is_some_and(|dir| {
            // The swap writes a new file next to the AppImage, so the folder must take one.
            let probe = dir.join(".mumcord-update-probe");
            let ok = std::fs::File::create(&probe).is_ok();
            if ok && let Err(e) = std::fs::remove_file(&probe) {
                tracing::warn!(error = %e, "removing the AppImage folder probe failed");
            }
            ok
        })
}

/// Detects the install type of the running app.
#[cfg(target_os = "linux")]
pub fn detect() -> InstallKind {
    let appimage = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty());
    let exists = |p: &str| Path::new(p).exists();
    linux_kind(&LinuxFacts {
        appimage: appimage.is_some(),
        appimage_writable: appimage.is_some_and(|p| writable(Path::new(&p))),
        arch: exists("/etc/arch-release"),
        dpkg_owns_us: exists("/var/lib/dpkg/info/mumcord.list"),
        rpm: exists("/usr/bin/rpm") || exists("/bin/rpm"),
        apt_repo: exists("/etc/apt/sources.list.d/mumcord.sources")
            || exists("/etc/apt/sources.list.d/mumcord.list"),
        dnf_repo: exists("/etc/yum.repos.d/mumcord.repo"),
        zypper_repo: exists("/etc/zypp/repos.d/mumcord.repo"),
        pacman_repo: pacman_conf_has_mumcord(),
    })
}

#[cfg(target_os = "linux")]
fn pacman_conf_has_mumcord() -> bool {
    match std::fs::read_to_string("/etc/pacman.conf") {
        Ok(conf) => has_mumcord_section(&conf),
        Err(e) => {
            tracing::debug!(error = %e, "reading /etc/pacman.conf failed");
            false
        }
    }
}

/// A `[mumcord]` section header, ignoring surrounding whitespace.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn has_mumcord_section(conf: &str) -> bool {
    conf.lines().any(|line| line.trim() == "[mumcord]")
}

#[cfg(target_os = "windows")]
pub fn detect() -> InstallKind {
    if crate::portable::is_active() {
        InstallKind::WindowsPortable
    } else {
        InstallKind::WindowsInstaller
    }
}

#[cfg(target_os = "macos")]
pub fn detect() -> InstallKind {
    let in_bundle = std::env::current_exe().is_ok_and(|exe| is_in_app_bundle(&exe));
    if in_bundle {
        InstallKind::MacApp
    } else {
        InstallKind::OtherMac
    }
}

/// `…/Mumcord.app/Contents/MacOS/mumcord`.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn is_in_app_bundle(exe: &Path) -> bool {
    exe.ancestors()
        .nth(3)
        .and_then(Path::extension)
        .is_some_and(|ext| ext == "app")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> LinuxFacts {
        LinuxFacts {
            appimage: false,
            appimage_writable: false,
            arch: false,
            dpkg_owns_us: false,
            rpm: false,
            apt_repo: false,
            dnf_repo: false,
            zypper_repo: false,
            pacman_repo: false,
        }
    }

    #[test]
    fn appimage_updates_itself_only_when_writable() {
        let writable = LinuxFacts {
            appimage: true,
            appimage_writable: true,
            ..facts()
        };
        assert_eq!(linux_kind(&writable), InstallKind::AppImage);
        let read_only = LinuxFacts {
            appimage: true,
            dpkg_owns_us: true,
            ..facts()
        };
        assert_eq!(linux_kind(&read_only), InstallKind::AppImageReadOnly);
    }

    #[test]
    fn packages_are_left_to_the_package_manager() {
        let arch = LinuxFacts {
            arch: true,
            rpm: true,
            ..facts()
        };
        assert_eq!(linux_kind(&arch), InstallKind::Aur);
        let deb = LinuxFacts {
            dpkg_owns_us: true,
            ..facts()
        };
        assert_eq!(linux_kind(&deb), InstallKind::Deb);
        let rpm = LinuxFacts {
            rpm: true,
            ..facts()
        };
        assert_eq!(linux_kind(&rpm), InstallKind::Rpm);
        assert_eq!(linux_kind(&facts()), InstallKind::OtherLinux);
    }

    #[test]
    fn repository_installs_get_their_package_manager() {
        let apt = LinuxFacts {
            dpkg_owns_us: true,
            apt_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&apt), InstallKind::AptRepo);
        let dnf = LinuxFacts {
            rpm: true,
            dnf_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&dnf), InstallKind::DnfRepo);
        let zypper = LinuxFacts {
            rpm: true,
            zypper_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&zypper), InstallKind::ZypperRepo);
        let pacman = LinuxFacts {
            arch: true,
            pacman_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&pacman), InstallKind::PacmanRepo);
    }

    #[test]
    fn repository_files_without_the_matching_package_do_not_count() {
        let apt_file_only = LinuxFacts {
            apt_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&apt_file_only), InstallKind::OtherLinux);
        let pacman_on_other = LinuxFacts {
            pacman_repo: true,
            rpm: true,
            ..facts()
        };
        assert_eq!(linux_kind(&pacman_on_other), InstallKind::Rpm);
        let appimage = LinuxFacts {
            appimage: true,
            appimage_writable: true,
            dpkg_owns_us: true,
            apt_repo: true,
            ..facts()
        };
        assert_eq!(linux_kind(&appimage), InstallKind::AppImage);
    }

    #[test]
    fn pacman_section_is_found_by_its_header() {
        assert!(has_mumcord_section(
            "[options]\n\n  [mumcord]  \nServer = x\n"
        ));
        assert!(!has_mumcord_section("#[mumcord]\n[mumcord-extra]\n"));
    }

    #[test]
    fn only_self_updating_kinds_have_a_manifest_target() {
        assert!(
            InstallKind::AppImage
                .manifest_target()
                .is_some_and(|t| t.ends_with("-appimage"))
        );
        for kind in [
            InstallKind::AppImageReadOnly,
            InstallKind::WindowsPortable,
            InstallKind::Deb,
            InstallKind::Rpm,
            InstallKind::Aur,
            InstallKind::AptRepo,
            InstallKind::DnfRepo,
            InstallKind::ZypperRepo,
            InstallKind::PacmanRepo,
            InstallKind::OtherLinux,
            InstallKind::OtherMac,
        ] {
            assert_eq!(kind.manifest_target(), None, "{kind:?}");
        }
    }

    #[test]
    fn app_bundle_is_recognised() {
        assert!(is_in_app_bundle(Path::new(
            "/Applications/Mumcord.app/Contents/MacOS/mumcord"
        )));
        assert!(!is_in_app_bundle(Path::new("/usr/local/bin/mumcord")));
    }
}
