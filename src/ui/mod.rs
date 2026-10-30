//! Drawing the whole frame.

pub mod layout;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::theme::{self, color};
use crate::ui::layout::{ReaderLayout, TokenKind, VisualLine};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.term_cols = area.width;
    app.term_rows = area.height;

    if area.width < 20 || area.height < 8 {
        return;
    }

    let bg = color(app.palette.bg);
    f.render_widget(Block::default().style(Style::default().bg(bg)), area);

    if app.is_booting() {
        draw_boot(f, app, area);
        return;
    }

    draw_chrome(f, app, area);
    draw_reader(f, app, area);
    draw_progress(f, app, area);
    draw_status(f, app, area);

    if app.help {
        draw_help(f, app, area);
    }
    if let Some(msg) = &app.error {
        draw_error(f, app, area, msg);
    }
}

fn draw_chrome(f: &mut Frame, app: &App, area: Rect) {
    let cols = area.width as usize;
    let rows = area.height as usize;
    if cols < 20 || rows < 8 {
        return;
    }
    let p = app.palette;
    let faint = color(p.faint);
    let cyan = color(p.cyan);
    let dim = color(p.dim);
    let rule = Style::default().fg(faint).bg(color(p.bg));

    // Top rule with the wordmark interrupting it.
    let mut spans = Vec::new();
    spans.push(Span::styled("\u{2500}".repeat(3), rule));
    spans.push(Span::styled(
        " voices ",
        Style::default().fg(cyan).bg(color(p.bg)).add_modifier(Modifier::BOLD),
    ));
    let version = format!("{} ", env!("CARGO_PKG_VERSION"));
    spans.push(Span::styled(version, Style::default().fg(dim).bg(color(p.bg))));
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let tail = cols.saturating_sub(used);
    spans.push(Span::styled("\u{2500}".repeat(tail), rule));
    f.render_widget(Line::from(spans), Rect::new(0, 0, area.width, 1));

    // Side rails.
    for y in 1..rows.saturating_sub(4) {
        let cell = Rect::new(0, y as u16, 1, 1);
        f.render_widget(Span::styled("\u{2502}", Style::default().fg(faint).bg(color(p.bg))), cell);
        let cell = Rect::new((cols - 1) as u16, y as u16, 1, 1);
        f.render_widget(Span::styled("\u{2502}", Style::default().fg(faint).bg(color(p.bg))), cell);
    }

    // Top-right document label.
    let title = truncate(&app.doc.title, 34);
    let label = format!(" {}  \u{00b7}  {}  \u{00b7}  {} ", title, app.language.short(), app.voice_short());
    let lx = cols.saturating_sub(label.chars().count() + 2);
    if lx > 12 {
        f.render_widget(
            Span::styled(label, Style::default().fg(color(p.quiet())).bg(color(p.bg))),
            Rect::new(lx as u16, 0, (cols - lx) as u16, 1),
        );
    }
}

fn draw_reader(f: &mut Frame, app: &App, area: Rect) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let h = app.visible_height();
    let top = app.scroll.round() as usize;
    let text_x = 4usize;
    let text_w = app.layout_width;
    let center_x = (cols.saturating_sub(text_w)) / 2;

    // Current sentence vertical bar, spanning its visual lines.
    if let Some((first, last)) = app.layout.sentence_lines(app.current_sentence) {
        for line_idx in first..=last {
            if line_idx < top || line_idx >= top + h {
                continue;
            }
            let y = 2 + (line_idx - top);
            if y + 1 > rows {
                break;
            }
            let pulse = crate::app::pulse(app.started.elapsed().as_secs_f64());
            let bar_color = color(theme::mix(p.bg, p.cyan, pulse));
            f.render_widget(
                Span::styled("\u{258c}", Style::default().fg(bar_color).bg(color(p.bg))),
                Rect::new(1, y as u16, 1, 1),
            );
        }
    }

    for row in 0..h {
        let idx = top + row;
        if idx >= app.layout.lines.len() {
            break;
        }
        let line = &app.layout.lines[idx];
        let y = 2 + row;
        let mut spans: Vec<Span> = Vec::new();
        for token in &line.tokens {
            let style = token_style(app, token, line);
            spans.push(Span::styled(token.text.clone(), style));
        }
        f.render_widget(
            Line::from(spans),
            Rect::new(center_x as u16, y as u16, text_w as u16, 1),
        );
    }
}

fn token_style(app: &App, token: &crate::ui::layout::Token, line: &VisualLine) -> Style {
    use crate::ui::layout::TokenKind;
    let p = app.palette;
    match &token.kind {
        TokenKind::Prefix => Style::default().fg(color(p.dim)).bg(color(p.bg)),
        TokenKind::Space => Style::default().bg(color(p.bg)),
        TokenKind::Word(w) => {
            let sentence = app.layout.word_sentence.get(*w).copied().unwrap_or(0);
            if Some(*w) == app.current_word {
                Style::default()
                    .fg(color(p.current_fg))
                    .bg(color(p.current_bg))
                    .add_modifier(Modifier::BOLD)
            } else if app.current_word.map(|c| *w < c).unwrap_or(false) {
                Style::default().fg(color(p.spoken)).bg(color(p.bg))
            } else if sentence < app.current_sentence {
                Style::default().fg(color(p.spoken)).bg(color(p.bg))
            } else if sentence == app.current_sentence {
                Style::default().fg(color(p.fg)).bg(color(p.bg))
            } else {
                Style::default().fg(color(p.upcoming)).bg(color(p.bg))
            }
        }
    }
}

fn draw_progress(f: &mut Frame, app: &App, area: Rect) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let bar_row = rows.saturating_sub(4);
    let tick_row = rows.saturating_sub(3);
    let bar_x = 2usize;
    let bar_w = cols.saturating_sub(4).max(8);
    let frac = if app.speaker.is_some() {
        app.snapshot.total_progress
    } else {
        0.0
    };

    // Track + fill, as background-coloured cells (WORDS style).
    let mut spans = Vec::new();
    let fill = ((bar_w as f32 * frac).round() as usize).min(bar_w);
    let track_style = Style::default().bg(color(p.faint)).fg(color(p.faint));
    let fill_style = Style::default().bg(color(p.cyan)).fg(color(p.cyan));
    let head_style = Style::default().bg(color(p.fg)).fg(color(p.fg));
    // Build one string of spaces; styling is per-span.
    let mut x = bar_x;
    // left cap
    spans.push(Span::styled(" ", Style::default().fg(color(p.dim)).bg(color(p.bg))));
    // track
    if fill == 0 {
        spans.push(Span::styled(" ".repeat(bar_w), track_style));
    } else {
        spans.push(Span::styled(" ".repeat(fill.saturating_sub(1)), fill_style));
        spans.push(Span::styled(" ", head_style));
        if fill < bar_w {
            spans.push(Span::styled(" ".repeat(bar_w - fill), track_style));
        }
    }
    spans.push(Span::styled(" ", Style::default().fg(color(p.dim)).bg(color(p.bg))));
    f.render_widget(Line::from(spans), Rect::new(0, bar_row as u16, area.width, 1));

    // Ticks, rendered as a string with spaces.
    let mut tick_line = vec![' '; cols];
    for i in 0..=10 {
        let pos = bar_x + (bar_w * i / 10).min(bar_w.saturating_sub(1));
        if pos < cols {
            tick_line[pos] = '\u{00b7}';
        }
    }
    let tick_str: String = tick_line.into_iter().collect();
    f.render_widget(
        Span::styled(tick_str, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new(0, tick_row as u16, area.width, 1),
    );
    let _ = x;
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let y = rows.saturating_sub(1);
    let divider_y = rows.saturating_sub(2);
    let divider: String = "\u{2500}".repeat(cols);
    f.render_widget(
        Span::styled(divider, Style::default().fg(color(p.faint)).bg(color(p.bg))),
        Rect::new(0, divider_y as u16, area.width, 1),
    );

    // Build the left field list.
    let status_label = match app.snapshot.status {
        crate::tts::Status::Playing => ("PLAYING", p.green),
        crate::tts::Status::Paused => ("PAUSED", p.amber),
        crate::tts::Status::Buffering => ("BUFFERING", p.magenta),
        crate::tts::Status::Stopped => ("STOPPED", p.dim),
        crate::tts::Status::Finished => ("FINISHED", p.cyan),
        crate::tts::Status::Error => ("ERROR", p.magenta),
        crate::tts::Status::Booting => ("BOOTING", p.dim),
    };

    let mut left = String::new();
    let mut left_styles: Vec<(String, ratatui::style::Color)> = Vec::new();
    left_styles.push((status_label.0.to_string(), color(status_label.1)));
    if app.no_audio {
        left_styles.push(("  viewer".to_string(), color(p.dim)));
    } else {
        let wc = app.doc.word_count;
        let cur = app.current_word.map(|w| w + 1).unwrap_or(0);
        left_styles.push((format!("   word {cur}/{wc}"), color(p.fg)));
        let sc = app.current_sentence + 1;
        let st = app.doc.sentences.len();
        left_styles.push((format!("   sent {sc}/{st}"), color(p.fg)));
        left_styles.push((format!("   {:.1}x", app.speed), color(p.amber)));
    }
    if let Some((msg, _)) = &app.notice {
        left_styles.push((format!("   {msg}"), color(p.green)));
    } else if let Some(msg) = &app.snapshot.message {
        left_styles.push((format!("   {msg}"), color(p.magenta)));
    }

    // Right side.
    let right = format!(
        "{} \u{00b7} {} \u{00b7} {}  ",
        app.language.short(),
        app.voice_short(),
        format_session(app.session_start.elapsed().as_secs())
    );
    let right_style = Style::default().fg(color(p.cyan)).bg(color(p.bg));

    // Key hints in the gap.
    let hints = "space pause  \u{2190}/\u{2192} sentence  \u{2191}/\u{2193} speed  s stop  t theme  ? help  q quit";

    // Render left fields.
    let mut x = 2usize;
    for (text, c) in &left_styles {
        let span = Span::styled(text.clone(), Style::default().fg(*c).bg(color(p.bg)));
        let w = text.chars().count();
        f.render_widget(span, Rect::new(x as u16, y as u16, w as u16, 1));
        x += w;
    }

    // Hints if room.
    let right_w = right.chars().count();
    let rx = cols.saturating_sub(right_w + 2);
    if rx > x + 16 {
        let avail = rx.saturating_sub(x + 2);
        let h: String = hints.chars().take(avail).collect();
        let hw = h.chars().count();
        f.render_widget(
            Span::styled(h, Style::default().fg(color(p.dim)).bg(color(p.bg))),
            Rect::new((x + 2) as u16, y as u16, hw as u16, 1),
        );
    }

    // Right.
    f.render_widget(
        Span::styled(right, right_style),
        Rect::new(rx as u16, y as u16, right_w as u16, 1),
    );
}

fn draw_boot(f: &mut Frame, app: &App, area: Rect) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let t = app.boot;
    let title = "VOICES";
    let tx = (cols.saturating_sub(title.len())) / 2;
    let ty = rows.saturating_sub(4) / 2;
    f.render_widget(
        Span::styled(
            title,
            Style::default()
                .fg(color(p.cyan))
                .bg(color(p.bg))
                .add_modifier(Modifier::BOLD),
        ),
        Rect::new(tx as u16, ty as u16, title.len() as u16, 1),
    );
    let sub = "text to speech \u{00b7} piper neural voices";
    let sx = (cols.saturating_sub(sub.len())) / 2;
    f.render_widget(
        Span::styled(sub, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new(sx as u16, (ty + 1) as u16, sub.len() as u16, 1),
    );
    let bar_w = (cols as f64 * 0.36).round() as usize;
    let bx = (cols - bar_w) / 2;
    let by = ty + 3;
    let fill = (bar_w as f64 * t).round() as usize;
    f.render_widget(
        Span::styled(" ".repeat(bar_w), Style::default().bg(color(p.faint))),
        Rect::new(bx as u16, by as u16, bar_w as u16, 1),
    );
    if fill > 0 {
        f.render_widget(
            Span::styled(" ".repeat(fill), Style::default().bg(color(p.cyan))),
            Rect::new(bx as u16, by as u16, fill as u16, 1),
        );
    }
    let msg = "INITIALISING";
    let mx = (cols - msg.len()) / 2;
    f.render_widget(
        Span::styled(msg, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new(mx as u16, (by + 2) as u16, msg.len() as u16, 1),
    );
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let items: &[(&str, &str)] = &[
        ("SPACE", "pause / resume"),
        ("\u{2190} / \u{2192}", "previous / next sentence"),
        ("\u{2191} / \u{2193}", "faster / slower"),
        ("S", "stop and rewind the sentence"),
        ("T", "toggle dark / light theme"),
        ("?", "close this panel"),
        ("Q / ESC", "quit"),
    ];
    let mut info: Vec<(&str, &str)> = Vec::new();
    info.push(("LANG", app.language.label()));
    info.push(("VOICE", &app.voice));
    info.push(("STATUS", status_name(app.snapshot.status)));
    let w = 46usize.min(cols.saturating_sub(2));
    let h = (items.len() + info.len() + 6).min(rows.saturating_sub(2));
    let x = (cols.saturating_sub(w)) / 2;
    let y = (rows.saturating_sub(h)) / 2;
    let rect = Rect::new(x as u16, y as u16, w as u16, h as u16);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color(p.cyan)))
        .style(Style::default().bg(color(p.bg)));
    f.render_widget(block, rect);

    let title = "CONTROLS";
    f.render_widget(
        Span::styled(
            title,
            Style::default()
                .fg(color(p.fg))
                .bg(color(p.bg))
                .add_modifier(Modifier::BOLD),
        ),
        Rect::new((x + (w - title.len()) / 2) as u16, (y + 1) as u16, title.len() as u16, 1),
    );

    let mut ry = y + 3;
    for (k, v) in items {
        render_kv(f, app.palette, x, ry, w, k, v);
        ry += 1;
    }
    ry += 1;
    for (k, v) in &info {
        render_kv(f, app.palette, x, ry, w, k, v);
        ry += 1;
    }
    let hint = "space pause \u{00b7} \u{2190}/\u{2192} sentence \u{00b7} \u{2191}/\u{2193} speed \u{00b7} ? close";
    let hx = x + (w.saturating_sub(hint.len())) / 2;
    f.render_widget(
        Span::styled(hint, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new(hx as u16, (y + h - 2) as u16, hint.len() as u16, 1),
    );
}

fn render_kv(f: &mut Frame, p: crate::theme::Palette, x: usize, y: usize, w: usize, k: &str, v: &str) {
    let kk = format!("{:<10}", k);
    f.render_widget(
        Span::styled(kk, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new((x + 2) as u16, y as u16, 10, 1),
    );
    let vx = x + w - 2 - v.chars().count();
    f.render_widget(
        Span::styled(v.to_string(), Style::default().fg(color(p.fg)).bg(color(p.bg))),
        Rect::new(vx as u16, y as u16, v.chars().count() as u16, 1),
    );
}

fn draw_error(f: &mut Frame, app: &App, area: Rect, msg: &str) {
    let p = app.palette;
    let cols = area.width as usize;
    let rows = area.height as usize;
    let w = 60usize.min(cols.saturating_sub(4));
    let lines = wrap(msg, w.saturating_sub(4));
    let h = (lines.len() + 4).min(rows.saturating_sub(2));
    let x = (cols.saturating_sub(w)) / 2;
    let y = (rows.saturating_sub(h)) / 2;
    let rect = Rect::new(x as u16, y as u16, w as u16, h as u16);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color(p.magenta)))
        .style(Style::default().bg(color(p.bg)));
    f.render_widget(block, rect);
    let title = "AUDIO ERROR";
    f.render_widget(
        Span::styled(title, Style::default().fg(color(p.magenta)).bg(color(p.bg)).add_modifier(Modifier::BOLD)),
        Rect::new((x + (w - title.len()) / 2) as u16, (y + 1) as u16, title.len() as u16, 1),
    );
    for (i, line) in lines.iter().enumerate() {
        f.render_widget(
            Span::styled(line.clone(), Style::default().fg(color(p.fg)).bg(color(p.bg))),
            Rect::new((x + 2) as u16, (y + 2 + i) as u16, line.chars().count() as u16, 1),
        );
    }
    let hint = "space to retry \u{00b7} q to quit";
    f.render_widget(
        Span::styled(hint, Style::default().fg(color(p.dim)).bg(color(p.bg))),
        Rect::new((x + (w - hint.len()) / 2) as u16, (y + h - 2) as u16, hint.len() as u16, 1),
    );
}

// -- helpers ----------------------------------------------------------------

impl App {
    pub fn voice_short(&self) -> String {
        let name = self.voice.to_uppercase();
        name.chars().take(14).collect()
    }
}

fn status_name(s: crate::tts::Status) -> &'static str {
    match s {
        crate::tts::Status::Playing => "PLAYING",
        crate::tts::Status::Paused => "PAUSED",
        crate::tts::Status::Buffering => "BUFFERING",
        crate::tts::Status::Stopped => "STOPPED",
        crate::tts::Status::Finished => "FINISHED",
        crate::tts::Status::Error => "ERROR",
        crate::tts::Status::Booting => "BOOTING",
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    let mut w = 0usize;
    for word in text.split_whitespace() {
        let wl = word.chars().count();
        if w + wl + usize::from(!line.is_empty()) > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            w = 0;
        }
        if !line.is_empty() {
            line.push(' ');
            w += 1;
        }
        line.push_str(word);
        w += wl;
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn format_session(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}
