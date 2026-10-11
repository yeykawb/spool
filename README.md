# spool

A minimal Windows app for saving audio as MP3. Paste a link to a video or a playlist and spool downloads each track with [yt-dlp](https://github.com/yt-dlp/yt-dlp) and converts it with [ffmpeg](https://ffmpeg.org). Written in Rust with [iced](https://iced.rs).

- Dark by default. The moon/sun icon switches to day mode, and the choice is remembered.
- Playlists are split into one row per track. Three tracks download at a time.
- Click the quality to cycle 320 / 256 / 192 / 128 kbps. Click the folder to change where files go (default: `Music\spool`).

## yt-dlp and ffmpeg

spool does not bundle either tool. On every launch it:

1. Uses its own copies in `%LOCALAPPDATA%\spool\data\bin` if they exist, otherwise looks on `PATH`.
2. Checks the newest release (yt-dlp from GitHub, ffmpeg from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/)).
3. Shows **Install** if a tool is missing, or **Update** if a newer version is out.

Install and Update both download into spool's own folder, and that copy is used from then on, even if an older one sits on `PATH`. ffmpeg builds that are not plain release numbers (git builds like `N-118000-g…`) are never flagged as outdated.

YouTube changes often, so keep yt-dlp current. Recent yt-dlp versions also need a JavaScript runtime such as [Deno](https://deno.com) for full YouTube support. spool does not install that for you.

## Build

Needs Rust (stable) on Windows:

```
cargo run --release
```

The exe ends up in `target\release\spool.exe`. CI also builds it on every push; grab `spool.exe` from the workflow run's artifacts.

## Layout

- `crates/core`: finding, checking and installing the tools, resolving links, running downloads. No UI.
- `crates/gui`: the iced app.
