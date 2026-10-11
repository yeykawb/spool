//! Locating yt-dlp, ffmpeg and Deno, reading their versions, and checking for
//! newer releases.
//!
//! Tools are never bundled. spool uses its own copies in the app-data
//! folder when present, otherwise whatever is on PATH. "Install" and
//! "Update" both download a fresh copy into the app-data folder, which
//! then takes precedence over PATH.

use std::path::{Path, PathBuf};

use crate::{command, Result};

const YT_DLP_LATEST_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const YT_DLP_EXE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const FFMPEG_LATEST_URL: &str = "https://www.gyan.dev/ffmpeg/builds/release-version";
const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
const DENO_LATEST_API: &str = "https://api.github.com/repos/denoland/deno/releases/latest";
const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const USER_AGENT: &str = concat!("spool/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    /// JavaScript runtime yt-dlp needs for full YouTube support. Optional:
    /// downloads still run without it.
    Deno,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::YtDlp, Tool::Ffmpeg, Tool::Deno];

    pub fn name(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
        }
    }

    fn file_name(self) -> String {
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub path: PathBuf,
    pub version: String,
    /// True when this is spool's own copy in the app-data folder.
    pub managed: bool,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub tool: Tool,
    pub installed: Option<Installed>,
    /// Set when the tool is present but would not run, for example because
    /// antivirus blocked it. Installing again would not help, so the UI
    /// offers a re-check instead.
    pub problem: Option<String>,
    /// Newest released version, or None when the check failed (offline).
    pub latest: Option<String>,
}

impl Status {
    pub fn update_available(&self) -> bool {
        match (&self.installed, &self.latest) {
            (Some(installed), Some(latest)) => is_newer(latest, &installed.version),
            _ => false,
        }
    }
}

/// Where spool keeps its own copies of the tools.
pub fn managed_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "spool")
        .map(|dirs| dirs.data_local_dir().join("bin"))
        .unwrap_or_else(|| std::env::temp_dir().join("spool").join("bin"))
}

/// Finds every tool and checks for newer releases.
pub async fn check() -> Vec<Status> {
    futures::future::join_all(Tool::ALL.map(check_one)).await
}

async fn check_one(tool: Tool) -> Status {
    let (found, latest) = futures::join!(find(tool), latest_version(tool));
    let (installed, problem) = match found {
        Ok(installed) => (installed, None),
        Err(problem) => (None, Some(problem)),
    };
    Status {
        tool,
        installed,
        problem,
        latest: latest.ok(),
    }
}

/// Ok(None) means the tool is nowhere to be found. Err means a copy exists
/// but none of them would run.
async fn find(tool: Tool) -> Result<Option<Installed>> {
    let managed = managed_dir().join(tool.file_name());
    let candidates = [
        managed.is_file().then_some((managed, true)),
        which::which(tool.name()).ok().map(|p| (p, false)),
    ];

    let mut failure = None;
    for (path, managed) in candidates.into_iter().flatten() {
        match installed_version(tool, &path).await {
            Ok(version) => {
                return Ok(Some(Installed {
                    path,
                    version,
                    managed,
                }))
            }
            Err(e) => failure = Some(format!("{} would not start ({e}).", path.display())),
        }
    }
    failure.map_or(Ok(None), Err)
}

/// Runs the tool for its version. Retries once, since the first launch of a
/// big exe can fail or stall while antivirus scans it.
async fn installed_version(tool: Tool, path: &Path) -> Result<String> {
    match read_version(tool, path).await {
        Ok(v) => Ok(v),
        Err(_) => {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            read_version(tool, path).await
        }
    }
}

async fn read_version(tool: Tool, path: &Path) -> Result<String> {
    let flag = match tool {
        Tool::YtDlp | Tool::Deno => "--version",
        Tool::Ffmpeg => "-version",
    };
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        command(path).arg(flag).output(),
    )
    .await
    .map_err(|_| "timed out".to_string())?
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(match out.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "it was stopped".to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default().trim();
    match tool {
        Tool::YtDlp => Ok(first.to_string()),
        // "ffmpeg version 7.1.1-essentials_build-www.gyan.dev Copyright ..."
        Tool::Ffmpeg => first
            .split_whitespace()
            .nth(2)
            .map(|v| {
                v.trim_start_matches('n')
                    .split('-')
                    .next()
                    .unwrap_or(v)
                    .to_string()
            })
            .ok_or_else(|| format!("unexpected ffmpeg output: {first}")),
        // "deno 2.5.4 (stable, release, x86_64-pc-windows-msvc)"
        Tool::Deno => first
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .ok_or_else(|| format!("unexpected deno output: {first}")),
    }
}

async fn latest_version(tool: Tool) -> Result<String> {
    let client = http()?;
    match tool {
        Tool::YtDlp => github_tag(&client, YT_DLP_LATEST_API).await,
        Tool::Deno => Ok(github_tag(&client, DENO_LATEST_API)
            .await?
            .trim_start_matches('v')
            .to_string()),
        Tool::Ffmpeg => Ok(client
            .get(FFMPEG_LATEST_URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?
            .trim()
            .to_string()),
    }
}

async fn github_tag(client: &reqwest::Client, api: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let release: Release = client
        .get(api)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(release.tag_name)
}

/// True when `latest` is a strictly higher dotted version than `current`.
/// Versions that are not plain numbers (ffmpeg git builds like
/// "N-118000-g…") never count as outdated, so they are never nagged about.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        v.split('.').map(|p| p.parse().ok()).collect()
    }
    match (parts(latest), parts(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Downloads the newest release into the app-data folder.
pub async fn install(tool: Tool) -> Result<()> {
    if !cfg!(windows) {
        return Err(format!(
            "Automatic install only works on Windows. Install {} with your package manager.",
            tool.name()
        ));
    }
    let dir = managed_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;

    match tool {
        Tool::YtDlp => {
            let tmp = dir.join("yt-dlp.exe.part");
            download_to(YT_DLP_EXE_URL, &tmp).await?;
            replace(&tmp, &dir.join(tool.file_name())).await
        }
        Tool::Ffmpeg => install_zip(FFMPEG_ZIP_URL, &dir, &["ffmpeg.exe", "ffprobe.exe"]).await,
        Tool::Deno => install_zip(DENO_ZIP_URL, &dir, &["deno.exe"]).await,
    }
}

async fn install_zip(url: &str, dir: &Path, exes: &'static [&'static str]) -> Result<()> {
    let zip = dir.join(format!("{}.zip.part", exes[0]));
    download_to(url, &zip).await?;
    let (zip_path, out) = (zip.clone(), dir.to_path_buf());
    let result = tokio::task::spawn_blocking(move || extract(&zip_path, &out, exes))
        .await
        .map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(zip).await;
    result
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())
}

async fn download_to(url: &str, dest: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut resp = http()?
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Download failed: {e}"))?
    {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())
}

async fn replace(tmp: &Path, dest: &Path) -> Result<()> {
    tokio::fs::rename(tmp, dest).await.map_err(|e| {
        format!(
            "Could not replace {} ({e}). Wait for running downloads to finish and try again.",
            dest.display()
        )
    })
}

/// Pulls the named executables out of a release zip, wherever they sit in
/// it (gyan.dev nests them under `ffmpeg-<version>-essentials_build/bin/`,
/// Deno puts `deno.exe` at the root).
fn extract(zip_path: &Path, out_dir: &Path, exes: &[&str]) -> Result<()> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for &exe in exes {
        let name = archive
            .file_names()
            .find(|n| n.rsplit('/').next() == Some(exe))
            .map(str::to_string)
            .ok_or_else(|| format!("{exe} not found in the download"))?;
        let mut entry = archive.by_name(&name).map_err(|e| e.to_string())?;
        let tmp = out_dir.join(format!("{exe}.part"));
        let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        drop(out);
        std::fs::rename(&tmp, out_dir.join(exe)).map_err(|e| {
            format!("Could not replace {exe} ({e}). Wait for running downloads to finish and try again.")
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("2026.09.30", "2026.08.01"));
        assert!(!is_newer("2026.09.30", "2026.09.30"));
        assert!(!is_newer("2026.09.30", "2026.09.30.232813"));
        assert!(is_newer("7.1.1", "7.1"));
        assert!(!is_newer("7.1.1", "N-118000-g1234abcd"));
    }
}
