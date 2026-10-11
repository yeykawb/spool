//! Everything spool does that is not drawing pixels: finding and updating
//! yt-dlp and ffmpeg, and running downloads.

pub mod job;
pub mod tools;

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

pub type Result<T> = std::result::Result<T, String>;

/// A child process that never opens a console window on Windows and dies
/// with its handle, so cancelling a download also stops yt-dlp.
pub(crate) fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}
