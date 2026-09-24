//! Python through uv: a managed CPython in `runtime/python`, a virtual environment in
//! `runtime/pyenv`, and the data packages the assistant's analysis tool relies on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use demido_core::manifest::ToolInfo;

use crate::plan::StepId;
use crate::process::{self, Run};
use crate::runner::Ctx;

pub(crate) async fn install_python(ctx: &Ctx<'_>, uv: &ToolInfo) -> anyhow::Result<ToolInfo> {
    let spec = &ctx.catalog.runtimes.python;
    let venv = ctx.install_dir().join("runtime").join("pyenv");
    let python = venv_python(&venv);

    if python.is_file() {
        if let Ok(version) = python_version(ctx, &python).await {
            if version.starts_with(&spec.version) {
                ctx.log(StepId::Python, format!("Python {version} is already installed"));
                ctx.progress(StepId::Python, 1, Some(1), 0.0, "Already installed");
                return Ok(ToolInfo {
                    version,
                    exe: super::relative(ctx, &python),
                });
            }
        }
    }
    if venv.exists() {
        std::fs::remove_dir_all(&venv)
            .with_context(|| format!("clearing {}", venv.display()))?;
    }

    ctx.progress(StepId::Python, 0, None, 0.0, format!("Installing Python {}", spec.version));
    let uv_exe = ctx.install_dir().join(&uv.exe);
    process::run(
        Run {
            program: &uv_exe,
            args: vec![
                "venv".into(),
                venv.to_string_lossy().into_owned(),
                "--python".into(),
                spec.version.clone(),
                "--python-preference".into(),
                "only-managed".into(),
            ],
            envs: uv_env(ctx),
            cwd: Some(ctx.install_dir()),
            timeout: Duration::from_secs(15 * 60),
        },
        ctx.cancel,
        |line| {
            ctx.log(StepId::Python, line);
            ctx.progress(StepId::Python, 0, None, 0.0, line);
        },
    )
    .await
    .context("uv could not create the Python environment")?;

    if !python.is_file() {
        bail!("Python was not created at {}", python.display());
    }
    let version = python_version(ctx, &python).await?;
    ctx.progress(StepId::Python, 1, Some(1), 0.0, format!("Python {version} ready"));
    Ok(ToolInfo {
        version,
        exe: super::relative(ctx, &python),
    })
}

pub(crate) async fn install_packages(
    ctx: &Ctx<'_>,
    uv: &ToolInfo,
    python: &ToolInfo,
) -> anyhow::Result<()> {
    let spec = &ctx.catalog.runtimes.python;
    let uv_exe = ctx.install_dir().join(&uv.exe);
    let python_exe = ctx.install_dir().join(&python.exe);

    let mut args = vec![
        "pip".to_string(),
        "install".into(),
        "--python".into(),
        python_exe.to_string_lossy().into_owned(),
    ];
    args.extend(spec.packages.iter().cloned());
    ctx.progress(
        StepId::PythonPackages,
        0,
        None,
        0.0,
        format!("Installing {}", spec.packages.join(", ")),
    );
    process::run(
        Run {
            program: &uv_exe,
            args,
            envs: uv_env(ctx),
            cwd: Some(ctx.install_dir()),
            timeout: Duration::from_secs(20 * 60),
        },
        ctx.cancel,
        |line| {
            ctx.log(StepId::PythonPackages, line);
            ctx.progress(StepId::PythonPackages, 0, None, 0.0, line);
        },
    )
    .await
    .context("uv could not install the Python packages")?;

    let imports = spec
        .packages
        .iter()
        .map(|p| p.replace('-', "_"))
        .collect::<Vec<_>>()
        .join(", ");
    process::run(
        Run {
            program: &python_exe,
            args: vec!["-c".into(), format!("import {imports}; print('packages ok')")],
            envs: vec![],
            cwd: None,
            timeout: Duration::from_secs(120),
        },
        ctx.cancel,
        |line| ctx.log(StepId::PythonPackages, line),
    )
    .await
    .context("the packages were installed but do not import")?;
    ctx.progress(StepId::PythonPackages, 1, Some(1), 0.0, "Packages ready");
    Ok(())
}

fn uv_env(ctx: &Ctx<'_>) -> Vec<(String, String)> {
    let dir = ctx.install_dir();
    vec![
        (
            "UV_PYTHON_INSTALL_DIR".into(),
            dir.join("runtime").join("python").to_string_lossy().into_owned(),
        ),
        (
            "UV_CACHE_DIR".into(),
            ctx.downloads.join("uv-cache").to_string_lossy().into_owned(),
        ),
        ("UV_LINK_MODE".into(), "copy".into()),
        ("UV_NO_CONFIG".into(), "1".into()),
        ("UV_NO_PROGRESS".into(), "1".into()),
        ("UV_PYTHON_DOWNLOADS".into(), "automatic".into()),
        ("NO_COLOR".into(), "1".into()),
    ]
}

pub(crate) fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python3")
    }
}

async fn python_version(ctx: &Ctx<'_>, python: &Path) -> anyhow::Result<String> {
    let out = process::run(
        Run {
            program: python,
            args: vec![
                "-c".into(),
                "import platform; print(platform.python_version())".into(),
            ],
            envs: vec![],
            cwd: None,
            timeout: Duration::from_secs(60),
        },
        ctx.cancel,
        |_| {},
    )
    .await?;
    Ok(out.trim().to_string())
}
