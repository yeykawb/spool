//! Turning a pasted link into tracks and downloading each one as MP3.

use std::path::PathBuf;

use futures::channel::mpsc::Sender;
use futures::SinkExt;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

use crate::{command, Result};

/// One downloadable track. A playlist link resolves to many.
#[derive(Debug, Clone)]
pub struct Entry {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub yt_dlp: PathBuf,
    /// Folder holding ffmpeg. None lets yt-dlp search PATH itself.
    pub ffmpeg_dir: Option<PathBuf>,
    /// Deno for YouTube's JavaScript challenges. None leaves yt-dlp to its
    /// defaults, which still work for most other sites.
    pub deno: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone)]
pub enum Progress {
    Downloading(f32),
    Converting,
    Done(PathBuf),
    Failed(String),
}

const SEP: char = '\u{1f}';

/// Lists the tracks behind a link without downloading anything.
pub async fn resolve(yt_dlp: PathBuf, url: String) -> Result<Vec<Entry>> {
    let out = command(&yt_dlp)
        .args([
            "--flat-playlist",
            "--no-warnings",
            "--encoding",
            "utf-8",
            "--print",
        ])
        .arg(format!("%(webpage_url,url)s{SEP}%(title)s"))
        .arg("--")
        .arg(&url)
        .output()
        .await
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;

    if !out.status.success() {
        return Err(last_error(&String::from_utf8_lossy(&out.stderr))
            .unwrap_or_else(|| "yt-dlp could not read that link.".into()));
    }

    let entries: Vec<Entry> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (url, title) = line.split_once(SEP)?;
            let title = if title.is_empty() || title == "NA" {
                url
            } else {
                title
            };
            Some(Entry {
                url: url.to_string(),
                title: title.to_string(),
            })
        })
        .collect();

    if entries.is_empty() {
        Err("No tracks found at that link.".into())
    } else {
        Ok(entries)
    }
}

/// Downloads one track and converts it to MP3, reporting as it goes. The
/// last message is always `Done` or `Failed`. Dropping the future kills
/// yt-dlp.
pub async fn download(opts: Options, url: String, mut tx: Sender<Progress>) {
    let result = run(&opts, &url, tx.clone()).await;
    let last = match result {
        Ok(path) => Progress::Done(path),
        Err(e) => Progress::Failed(e),
    };
    let _ = tx.send(last).await;
}

async fn run(opts: &Options, url: &str, tx: Sender<Progress>) -> Result<PathBuf> {
    let mut cmd = command(&opts.yt_dlp);
    cmd.args(["-x", "--audio-format", "mp3", "--audio-quality"])
        .arg(format!("{}K", opts.bitrate_kbps))
        .args([
            "--embed-metadata",
            "--no-playlist",
            "--encoding",
            "utf-8",
            "--newline",
            "--no-colors",
            "--no-warnings",
            "--quiet",
            "--progress",
            "--progress-template",
        ])
        .arg(format!(
            "download:SPOOL{SEP}%(progress.downloaded_bytes)s{SEP}%(progress.total_bytes,progress.total_bytes_estimate)s"
        ))
        .arg("--print")
        .arg(format!("after_move:SPOOLDONE{SEP}%(filepath)s"))
        .arg("-P")
        .arg(&opts.out_dir)
        .args(["-o", "%(title)s.%(ext)s"]);
    if let Some(dir) = &opts.ffmpeg_dir {
        cmd.arg("--ffmpeg-location").arg(dir);
    }
    if let Some(deno) = &opts.deno {
        cmd.arg("--js-runtimes")
            .arg(format!("deno:{}", deno.display()));
    }
    cmd.arg("--").arg(url);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    let stderr = BufReader::new(child.stderr.take().expect("piped stderr"));

    // yt-dlp splits its output between both streams depending on mode, so
    // read both the same way.
    let (a, b) = futures::join!(read(stdout, tx.clone()), read(stderr, tx));
    let status = child.wait().await.map_err(|e| e.to_string())?;

    let done = a.done.or(b.done);
    match (status.success(), done) {
        (true, Some(path)) => Ok(path),
        _ => Err(b
            .error
            .or(a.error)
            .unwrap_or_else(|| format!("yt-dlp stopped ({status}).",))),
    }
}

#[derive(Default)]
struct Outcome {
    done: Option<PathBuf>,
    error: Option<String>,
}

async fn read(mut reader: impl AsyncBufRead + Unpin, mut tx: Sender<Progress>) -> Outcome {
    let mut outcome = Outcome::default();
    let mut buf = Vec::new();
    let mut converting = false;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end();
        let mut fields = line.split(SEP);
        match fields.next() {
            Some("SPOOL") => {
                let done: f64 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let total: f64 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                if total > 0.0 {
                    let pct = (done / total * 100.0).clamp(0.0, 100.0) as f32;
                    if pct >= 100.0 && !converting {
                        converting = true;
                        let _ = tx.send(Progress::Converting).await;
                    } else if pct < 100.0 {
                        converting = false;
                        let _ = tx.send(Progress::Downloading(pct)).await;
                    }
                }
            }
            Some("SPOOLDONE") => outcome.done = fields.next().map(PathBuf::from),
            _ => {
                if let Some(e) = last_error(line) {
                    outcome.error = Some(e);
                }
            }
        }
    }
    outcome
}

fn last_error(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .find_map(|l| l.strip_prefix("ERROR: "))
        .map(|e| e.trim().to_string())
}
