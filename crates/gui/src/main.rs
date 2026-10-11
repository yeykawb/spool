#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;
mod style;

use std::path::PathBuf;

use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, progress_bar, row, scrollable,
    svg, text, text_input,
};
use iced::{task, Alignment, Element, Length, Size, Task, Theme};
use spool_core::job::{self, Entry, Progress};
use spool_core::tools::{self, Tool};

use settings::Settings;

/// Downloads running at once. More mostly gets you rate limited.
const PARALLEL: usize = 3;

const MOON: &[u8] = include_bytes!("../assets/moon.svg");
const SUN: &[u8] = include_bytes!("../assets/sun.svg");

fn main() -> iced::Result {
    iced::application("spool", App::update, App::view)
        .theme(App::theme)
        .window_size(Size::new(640.0, 520.0))
        .run_with(App::new)
}

struct App {
    settings: Settings,
    url: String,
    resolving: bool,
    error: Option<String>,
    jobs: Vec<Job>,
    next_id: u64,
    tools: Option<Vec<tools::Status>>,
    installing: Option<Tool>,
    tool_error: Option<String>,
    updates_dismissed: bool,
}

struct Job {
    id: u64,
    entry: Entry,
    state: JobState,
    handle: Option<task::Handle>,
}

#[derive(Debug, Clone, PartialEq)]
enum JobState {
    Queued,
    Downloading(f32),
    Converting,
    Done,
    Failed(String),
}

impl JobState {
    fn is_active(&self) -> bool {
        matches!(self, JobState::Downloading(_) | JobState::Converting)
    }
    fn is_finished(&self) -> bool {
        matches!(self, JobState::Done | JobState::Failed(_))
    }
}

#[derive(Debug, Clone)]
enum Message {
    UrlChanged(String),
    Submit,
    Resolved(Result<Vec<Entry>, String>),
    CycleBitrate,
    PickFolder,
    FolderPicked(Option<PathBuf>),
    ToggleTheme,
    Progress(u64, Progress),
    Remove(u64),
    ClearFinished,
    ToolsChecked(Vec<tools::Status>),
    Install(Tool),
    Installed(Result<(), String>),
    DismissUpdates,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            settings: Settings::load(),
            url: String::new(),
            resolving: false,
            error: None,
            jobs: Vec::new(),
            next_id: 0,
            tools: None,
            installing: None,
            tool_error: None,
            updates_dismissed: false,
        };
        (app, Task::perform(tools::check(), Message::ToolsChecked))
    }

    fn theme(&self) -> Theme {
        style::theme(self.settings.dark)
    }

    fn tool(&self, tool: Tool) -> Option<&tools::Status> {
        self.tools.as_ref()?.iter().find(|s| s.tool == tool)
    }

    fn installed(&self, tool: Tool) -> Option<&tools::Installed> {
        self.tool(tool)?.installed.as_ref()
    }

    fn ready(&self) -> bool {
        self.installed(Tool::YtDlp).is_some() && self.installed(Tool::Ffmpeg).is_some()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UrlChanged(url) => {
                self.url = url;
                self.error = None;
            }
            Message::Submit => {
                let url = self.url.trim().to_string();
                let Some(yt) = self.installed(Tool::YtDlp).map(|i| i.path.clone()) else {
                    return Task::none();
                };
                if url.is_empty() || self.resolving {
                    return Task::none();
                }
                self.resolving = true;
                self.error = None;
                return Task::perform(job::resolve(yt, url), Message::Resolved);
            }
            Message::Resolved(result) => {
                self.resolving = false;
                match result {
                    Ok(entries) => {
                        self.url.clear();
                        for entry in entries {
                            self.jobs.push(Job {
                                id: self.next_id,
                                entry,
                                state: JobState::Queued,
                                handle: None,
                            });
                            self.next_id += 1;
                        }
                        return self.start_queued();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Message::CycleBitrate => {
                self.settings.next_bitrate();
                self.settings.save();
            }
            Message::PickFolder => {
                let start = self.settings.out_dir.clone();
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_directory(start)
                            .pick_folder()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    Message::FolderPicked,
                );
            }
            Message::FolderPicked(Some(dir)) => {
                self.settings.out_dir = dir;
                self.settings.save();
            }
            Message::FolderPicked(None) => {}
            Message::ToggleTheme => {
                self.settings.dark = !self.settings.dark;
                self.settings.save();
            }
            Message::Progress(id, progress) => {
                if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
                    job.state = match progress {
                        Progress::Downloading(p) => JobState::Downloading(p),
                        Progress::Converting => JobState::Converting,
                        Progress::Done(_) => JobState::Done,
                        Progress::Failed(e) => JobState::Failed(e),
                    };
                    if job.state.is_finished() {
                        job.handle = None;
                        return self.start_queued();
                    }
                }
            }
            Message::Remove(id) => {
                if let Some(i) = self.jobs.iter().position(|j| j.id == id) {
                    let job = self.jobs.remove(i);
                    if let Some(handle) = job.handle {
                        handle.abort();
                    }
                    return self.start_queued();
                }
            }
            Message::ClearFinished => self.jobs.retain(|j| !j.state.is_finished()),
            Message::ToolsChecked(status) => {
                self.tools = Some(status);
                return self.start_queued();
            }
            Message::Install(tool) => {
                if self.installing.is_none() {
                    self.installing = Some(tool);
                    self.tool_error = None;
                    return Task::perform(tools::install(tool), Message::Installed);
                }
            }
            Message::Installed(result) => {
                self.installing = None;
                if let Err(e) = result {
                    self.tool_error = Some(e);
                }
                return Task::perform(tools::check(), Message::ToolsChecked);
            }
            Message::DismissUpdates => self.updates_dismissed = true,
        }
        Task::none()
    }

    /// Fills free download slots from the queue. Waits while a tool is
    /// being replaced, since Windows will not overwrite a running exe.
    fn start_queued(&mut self) -> Task<Message> {
        if self.installing.is_some() {
            return Task::none();
        }
        let (Some(yt), Some(ff)) = (self.installed(Tool::YtDlp), self.installed(Tool::Ffmpeg))
        else {
            return Task::none();
        };
        let opts = job::Options {
            yt_dlp: yt.path.clone(),
            ffmpeg_dir: ff.path.parent().map(PathBuf::from),
            out_dir: self.settings.out_dir.clone(),
            bitrate_kbps: self.settings.bitrate_kbps,
        };

        let running = self.jobs.iter().filter(|j| j.state.is_active()).count();
        let mut tasks = Vec::new();
        for job in self
            .jobs
            .iter_mut()
            .filter(|j| j.state == JobState::Queued)
            .take(PARALLEL.saturating_sub(running))
        {
            let (id, url, opts) = (job.id, job.entry.url.clone(), opts.clone());
            let stream = iced::stream::channel(32, move |tx| job::download(opts, url, tx));
            let (task, handle) = Task::run(stream, move |p| Message::Progress(id, p)).abortable();
            job.state = JobState::Downloading(0.0);
            job.handle = Some(handle);
            tasks.push(task);
        }
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        let mut page = column![].spacing(26);

        if let Some(notice) = self.tools_notice() {
            page = page.push(notice);
        }

        let input = text_input("Paste a link to a video or playlist", &self.url)
            .on_input(Message::UrlChanged)
            .on_submit(Message::Submit)
            .style(style::field)
            .size(14)
            .padding([6, 0]);
        let hint = if self.resolving {
            "Looking up…"
        } else if self.ready() {
            "Enter ↵"
        } else {
            ""
        };
        let field = column![
            row![input, text(hint).size(12).style(style::muted_text)]
                .spacing(12)
                .align_y(Alignment::Center),
            container(horizontal_space())
                .height(1.5)
                .style(style::underline),
        ]
        .spacing(4);

        let mut top = column![field].spacing(8);
        if let Some(e) = &self.error {
            top = top.push(text(e).size(12).style(style::danger_text));
        }
        page = page.push(top);

        let folder = self
            .settings
            .out_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.settings.out_dir.display().to_string());
        let icon = svg(svg::Handle::from_memory(if self.settings.dark {
            MOON
        } else {
            SUN
        }))
        .width(16)
        .height(16)
        .style(style::icon);
        let opts = row![
            option(
                "Quality",
                format!("{} kbps", self.settings.bitrate_kbps),
                Message::CycleBitrate
            ),
            option("Save to", folder, Message::PickFolder),
            horizontal_space(),
            button(icon)
                .padding(4)
                .style(style::link)
                .on_press(Message::ToggleTheme),
        ]
        .spacing(22)
        .align_y(Alignment::Center);
        page = page.push(opts);

        let list = self.jobs.iter().fold(column![], |col, job| {
            col.push(horizontal_rule(1).style(style::separator))
                .push(job_row(job))
        });
        page = page.push(scrollable(list).height(Length::Fill));

        page = page.push(self.footer());

        container(page).padding([30, 44]).into()
    }

    fn tools_notice(&self) -> Option<Element<'_, Message>> {
        let Some(statuses) = &self.tools else {
            return Some(
                text("Checking yt-dlp and ffmpeg…")
                    .size(13)
                    .style(style::muted_text)
                    .into(),
            );
        };

        let mut lines = column![].spacing(8);
        let mut any = false;
        let mut any_update = false;
        for status in statuses {
            let name = status.tool.name();
            let line: Option<(String, &str)> = match &status.installed {
                None => Some((format!("{name} is not installed."), "Install")),
                Some(i) if status.update_available() && !self.updates_dismissed => {
                    any_update = true;
                    let latest = status.latest.as_deref().unwrap_or_default();
                    Some((
                        format!("{name} {} → {latest} is available.", i.version),
                        "Update",
                    ))
                }
                _ => None,
            };
            let Some((label, action)) = line else {
                continue;
            };
            any = true;
            let control: Element<_> = if self.installing == Some(status.tool) {
                text("Downloading…")
                    .size(13)
                    .style(style::muted_text)
                    .into()
            } else {
                let mut b = button(text(action).size(13))
                    .padding([3, 10])
                    .style(style::action);
                if self.installing.is_none() {
                    b = b.on_press(Message::Install(status.tool));
                }
                b.into()
            };
            lines = lines.push(
                row![text(label).size(13), horizontal_space(), control]
                    .spacing(12)
                    .align_y(Alignment::Center),
            );
        }
        if let Some(e) = &self.tool_error {
            any = true;
            lines = lines.push(text(e).size(12).style(style::danger_text));
        }
        if !any {
            return None;
        }
        if any_update {
            lines = lines.push(
                row![
                    text("Updates go into spool's own folder and are used instead of copies on PATH.")
                        .size(12)
                        .style(style::muted_text),
                    horizontal_space(),
                    button(text("Later").size(12)).padding(0).style(style::link).on_press(Message::DismissUpdates),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        Some(
            container(lines)
                .padding([12, 14])
                .style(style::notice)
                .into(),
        )
    }

    fn footer(&self) -> Element<'_, Message> {
        let total = self.jobs.len();
        let done = self
            .jobs
            .iter()
            .filter(|j| j.state == JobState::Done)
            .count();
        let running = self.jobs.iter().filter(|j| j.state.is_active()).count();
        let failed = self
            .jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Failed(_)))
            .count();

        let mut summary = if total == 0 {
            String::from("Nothing queued")
        } else {
            format!("{done} of {total} done · {running} running")
        };
        if failed > 0 {
            summary.push_str(&format!(" · {failed} failed"));
        }

        let version = |t: Tool| {
            self.installed(t)
                .map(|i| format!("{} {}", t.name(), i.version))
                .unwrap_or_else(|| format!("{} missing", t.name()))
        };
        let versions = format!("{} · {}", version(Tool::YtDlp), version(Tool::Ffmpeg));

        let mut left = row![text(summary).size(12).style(style::muted_text)].spacing(14);
        if done + failed > 0 {
            left = left.push(
                button(text("Clear finished").size(12))
                    .padding(0)
                    .style(style::link)
                    .on_press(Message::ClearFinished),
            );
        }
        row![
            left,
            horizontal_space(),
            text(versions).size(12).style(style::muted_text)
        ]
        .align_y(Alignment::Center)
        .into()
    }
}

fn option(label: &'static str, value: String, on_press: Message) -> Element<'static, Message> {
    row![
        text(label).size(13).style(style::muted_text),
        button(text(value).size(13))
            .padding(0)
            .style(style::value)
            .on_press(on_press),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn job_row(job: &Job) -> Element<'_, Message> {
    let (fill, status) = match &job.state {
        JobState::Queued => (0.0, "Queued".to_string()),
        JobState::Downloading(p) => (*p, format!("{p:.0}%")),
        JobState::Converting => (100.0, "Converting".to_string()),
        JobState::Done => (100.0, "Done".to_string()),
        JobState::Failed(_) => (0.0, "Failed".to_string()),
    };

    let mut title = column![text(&job.entry.title)
        .size(14)
        .wrapping(text::Wrapping::None)];
    if let JobState::Failed(e) = &job.state {
        title = title.push(text(e).size(12).style(style::danger_text));
    }

    let remove = if job.state.is_active() {
        "Cancel"
    } else {
        "Remove"
    };
    let remove = iced::widget::tooltip(
        button(text("×").size(14))
            .padding([0, 4])
            .style(style::link)
            .on_press(Message::Remove(job.id)),
        text(remove).size(12),
        iced::widget::tooltip::Position::Left,
    );
    row![
        container(title).width(Length::Fill).clip(true),
        progress_bar(0.0..=100.0, fill)
            .width(90)
            .height(2)
            .style(style::progress),
        container(text(status).size(13).style(style::muted_text))
            .width(72)
            .align_x(Alignment::End),
        remove,
    ]
    .spacing(16)
    .padding([11, 0])
    .align_y(Alignment::Center)
    .into()
}
