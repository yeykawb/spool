//! The two palettes and the handful of widget styles built from them.
//! Everything is derived from background + text, so night and day stay in
//! step.

use iced::theme::Palette;
use iced::widget::{button, container, progress_bar, rule, svg, text_input};
use iced::{color, Background, Border, Color, Theme};

pub fn theme(dark: bool) -> Theme {
    let palette = if dark {
        Palette {
            background: color!(0x17181a),
            text: color!(0xe6e6e3),
            primary: color!(0xe6e6e3),
            success: color!(0x9cc79a),
            danger: color!(0xe08a80),
        }
    } else {
        Palette {
            background: color!(0xfbfbfa),
            text: color!(0x1d1d1b),
            primary: color!(0x1d1d1b),
            success: color!(0x3f7d3c),
            danger: color!(0xb3443a),
        }
    };
    Theme::custom(
        if dark { "spool night" } else { "spool day" }.into(),
        palette,
    )
}

fn fade(c: Color, alpha: f32) -> Color {
    Color { a: alpha, ..c }
}

pub fn muted(theme: &Theme) -> Color {
    fade(theme.palette().text, 0.58)
}

pub fn line(theme: &Theme) -> Color {
    fade(theme.palette().text, 0.12)
}

pub fn muted_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted(theme)),
    }
}

pub fn success_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().success),
    }
}

pub fn danger_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().danger),
    }
}

/// The URL field: no box, the rule underneath does the work.
pub fn field(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let p = theme.palette();
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: muted(theme),
        placeholder: muted(theme),
        value: p.text,
        selection: fade(p.text, 0.25),
    }
}

pub fn underline(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().text)
}

pub fn separator(theme: &Theme) -> rule::Style {
    rule::Style {
        color: line(theme),
        width: 1,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
    }
}

/// Plain text that is clickable: muted until hovered.
pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.palette();
    let text_color = match status {
        button::Status::Hovered | button::Status::Pressed => p.text,
        button::Status::Disabled => fade(p.text, 0.3),
        button::Status::Active => muted(theme),
    };
    button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..button::Style::default()
    }
}

/// Plain text value inside an option ("320 kbps"), full strength.
pub fn value(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.palette();
    button::Style {
        background: None,
        text_color: match status {
            button::Status::Disabled => fade(p.text, 0.3),
            _ => p.text,
        },
        border: Border::default(),
        ..button::Style::default()
    }
}

/// Outlined button for the few real actions (Install, Update).
pub fn action(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(fade(p.text, 0.08))),
        text_color: p.text,
        border: Border {
            color: fade(p.text, 0.5),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..button::Style::default()
    }
}

pub fn icon(theme: &Theme, status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(match status {
            svg::Status::Hovered => theme.palette().text,
            svg::Status::Idle => muted(theme),
        }),
    }
}

pub fn progress(theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(line(theme)),
        bar: Background::Color(theme.palette().text),
        border: Border::default(),
    }
}

pub fn notice(theme: &Theme) -> container::Style {
    container::Style::default()
        .background(fade(theme.palette().text, 0.05))
        .border(Border {
            color: line(theme),
            width: 1.0,
            radius: 4.0.into(),
        })
}
