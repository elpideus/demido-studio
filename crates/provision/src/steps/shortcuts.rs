//! Start menu and desktop shortcuts, and the entry in the OS's list of installed apps.

use crate::plan::StepId;
use crate::runner::Ctx;

pub(crate) async fn create(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let plan = ctx.plan.clone();
    let size = dir_size(&plan.install_dir);
    let notes = tokio::task::spawn_blocking(move || platform::create(&plan, size)).await??;
    for note in notes {
        ctx.log(StepId::Shortcuts, note);
    }
    Ok(())
}

fn dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

#[cfg(windows)]
pub(crate) mod platform {
    use std::path::{Path, PathBuf};

    use anyhow::Context;
    use demido_core::InstallScope;
    use demido_core::brand::{APP_NAME, PUBLISHER, STUDIO_BIN, UNINSTALL_KEY, UNINSTALLER_BIN, VERSION};
    use demido_core::platform::exe;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize, IPersistFile,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
        IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
    };
    use windows::core::{GUID, HSTRING, Interface};

    use crate::plan::InstallPlan;

    const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    pub(crate) fn create(plan: &InstallPlan, size: u64) -> anyhow::Result<Vec<String>> {
        let mut notes = Vec::new();
        let app = plan.install_dir.join(exe(STUDIO_BIN));
        if plan.shortcuts {
            for link in shortcut_paths(plan.scope)? {
                if let Some(parent) = link.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                write_shortcut(&link, &app, &plan.install_dir)
                    .with_context(|| format!("creating {}", link.display()))?;
                notes.push(format!("Created {}", link.display()));
            }
        }
        if plan.register {
            register(plan, size)?;
            notes.push(format!("Registered {APP_NAME} in Installed apps"));
        }
        Ok(notes)
    }

    /// Start menu entry and desktop shortcut for the scope.
    pub(crate) fn shortcut_paths(scope: InstallScope) -> anyhow::Result<Vec<PathBuf>> {
        let (programs, desktop) = match scope {
            InstallScope::User => (&FOLDERID_Programs, &FOLDERID_Desktop),
            InstallScope::Machine => (&FOLDERID_CommonPrograms, &FOLDERID_PublicDesktop),
        };
        let name = format!("{APP_NAME}.lnk");
        Ok(vec![
            known_folder(programs)?.join(&name),
            known_folder(desktop)?.join(&name),
        ])
    }

    fn known_folder(id: &GUID) -> anyhow::Result<PathBuf> {
        unsafe {
            let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
            let path = raw.to_string();
            CoTaskMemFree(Some(raw.0 as *const _));
            Ok(PathBuf::from(path?))
        }
    }

    fn write_shortcut(link: &Path, target: &Path, workdir: &Path) -> anyhow::Result<()> {
        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> windows::core::Result<()> {
                let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                shell.SetPath(&HSTRING::from(target.as_os_str()))?;
                shell.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
                shell.SetDescription(&HSTRING::from(
                    "An easy to use, powerful AI harness",
                ))?;
                shell.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
                let file: IPersistFile = shell.cast()?;
                file.Save(&HSTRING::from(link.as_os_str()), true)?;
                Ok(())
            })();
            if init.is_ok() {
                CoUninitialize();
            }
            result?;
        }
        Ok(())
    }

    fn register(plan: &InstallPlan, size: u64) -> anyhow::Result<()> {
        let root = match plan.scope {
            InstallScope::User => windows_registry::CURRENT_USER,
            InstallScope::Machine => windows_registry::LOCAL_MACHINE,
        };
        let key = root
            .create(format!(r"{UNINSTALL_ROOT}\{UNINSTALL_KEY}"))
            .context("opening the uninstall registry key")?;
        let dir = plan.install_dir.to_string_lossy().to_string();
        let app = plan.install_dir.join(exe(STUDIO_BIN));
        let uninstaller = plan.install_dir.join(exe(UNINSTALLER_BIN));
        key.set_string("DisplayName", APP_NAME)?;
        key.set_string("DisplayVersion", VERSION)?;
        key.set_string("Publisher", PUBLISHER)?;
        key.set_string("InstallLocation", &dir)?;
        key.set_string("DisplayIcon", app.to_string_lossy().as_ref())?;
        key.set_string(
            "UninstallString",
            &format!("\"{}\" --uninstall", uninstaller.display()),
        )?;
        key.set_string(
            "QuietUninstallString",
            &format!("\"{}\" --uninstall --quiet", uninstaller.display()),
        )?;
        key.set_string(
            "InstallDate",
            &chrono::Local::now().format("%Y%m%d").to_string(),
        )?;
        key.set_u32("NoModify", 1)?;
        key.set_u32("NoRepair", 1)?;
        key.set_u32("EstimatedSize", (size / 1024).min(u32::MAX as u64) as u32)?;
        Ok(())
    }

    pub(crate) fn unregister(scope: InstallScope) -> anyhow::Result<()> {
        let root = match scope {
            InstallScope::User => windows_registry::CURRENT_USER,
            InstallScope::Machine => windows_registry::LOCAL_MACHINE,
        };
        let path = format!(r"{UNINSTALL_ROOT}\{UNINSTALL_KEY}");
        if root.open(&path).is_ok() {
            root.remove_tree(&path)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod platform {
    use std::path::PathBuf;

    use demido_core::InstallScope;
    use demido_core::brand::{APP_NAME, APP_SLUG, STUDIO_BIN};

    use crate::plan::InstallPlan;

    pub(crate) fn create(plan: &InstallPlan, _size: u64) -> anyhow::Result<Vec<String>> {
        if !plan.shortcuts {
            return Ok(vec![]);
        }
        let mut notes = vec![];
        for path in shortcut_paths(plan.scope)? {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let exec = plan.install_dir.join(STUDIO_BIN);
            std::fs::write(
                &path,
                format!(
                    "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec=\"{}\"\nTerminal=false\nCategories=Utility;Development;\n",
                    exec.display()
                ),
            )?;
            notes.push(format!("Created {}", path.display()));
        }
        Ok(notes)
    }

    pub(crate) fn shortcut_paths(scope: InstallScope) -> anyhow::Result<Vec<PathBuf>> {
        let dir = match scope {
            InstallScope::User => dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from(".local/share"))
                .join("applications"),
            InstallScope::Machine => PathBuf::from("/usr/share/applications"),
        };
        Ok(vec![dir.join(format!("{APP_SLUG}.desktop"))])
    }

    pub(crate) fn unregister(_scope: InstallScope) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod platform {
    use std::path::PathBuf;

    use demido_core::InstallScope;

    use crate::plan::InstallPlan;

    pub(crate) fn create(_plan: &InstallPlan, _size: u64) -> anyhow::Result<Vec<String>> {
        Ok(vec!["Shortcuts are not needed on macOS".into()])
    }

    pub(crate) fn shortcut_paths(_scope: InstallScope) -> anyhow::Result<Vec<PathBuf>> {
        Ok(vec![])
    }

    pub(crate) fn unregister(_scope: InstallScope) -> anyhow::Result<()> {
        Ok(())
    }
}
