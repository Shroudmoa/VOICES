//! Application state shared by the UI and the speaker.

use std::time::Instant;

use crate::config::Config;
use crate::lang::Language;
use crate::text::Document;
use crate::theme::{Palette, ThemeName};
use crate::tts::{Snapshot, SpeakerHandle, Status};
use crate::ui::layout::ReaderLayout;

pub struct App {
    pub doc: Document,
    pub layout: ReaderLayout,
    pub theme: ThemeName,
    pub palette: Palette,
    pub term_cols: u16,
    pub term_rows: u16,
    pub scroll: f64,
    pub scroll_target: f64,
    pub help: bool,
    pub error_panel: bool,
    pub boot: f64,
    pub started: Instant,
    pub session_start: Instant,
    pub notice: Option<(String, Instant)>,
    pub speaker: Option<SpeakerHandle>,
    pub snapshot: Snapshot,
    pub no_audio: bool,
    pub language: Language,
    pub voice: String,
    pub speed: f32,
    pub quit: bool,
    pub current_sentence: usize,
    pub current_word: Option<usize>,
    pub local_sentence: usize,
    pub local_word: Option<usize>,
    pub layout_width: usize,
    pub error: Option<String>,
    pub config: Config,
}

impl App {
    pub fn new(
        doc: Document,
        theme: ThemeName,
        language: Language,
        voice: String,
        speed: f32,
        speaker: Option<SpeakerHandle>,
        no_audio: bool,
        config: Config,
        cols: u16,
        rows: u16,
    ) -> Self {
        let palette = Palette::of(theme);
        let width = usable_width(cols as usize);
        let layout = ReaderLayout::build(&doc, width);
        let snapshot = if let Some(s) = &speaker {
            s.snapshot()
        } else {
            Snapshot::idle(language, voice.clone(), speed, doc.sentences.len())
        };
        let mut app = App {
            doc,
            layout,
            theme,
            palette,
            term_cols: cols,
            term_rows: rows,
            scroll: 0.0,
            scroll_target: 0.0,
            help: false,
            error_panel: false,
            boot: 0.0,
            started: Instant::now(),
            session_start: Instant::now(),
            notice: None,
            speaker,
            snapshot,
            no_audio,
            language,
            voice,
            speed,
            quit: false,
            current_sentence: 0,
            current_word: None,
            local_sentence: 0,
            local_word: None,
            layout_width: width,
            error: None,
            config,
        };
        app.rebuild_if_needed();
        app
    }

    pub fn set_notice(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now()));
    }

    pub fn update(&mut self, dt: f64) {
        self.boot = (self.boot + dt).min(1.0);

        // Expire notices after a couple of seconds.
        if let Some((_, when)) = &self.notice {
            if when.elapsed().as_secs_f64() > 2.2 {
                self.notice = None;
            }
        }

        if let Some(s) = &self.speaker {
            self.snapshot = s.snapshot();
        }

        if self.speaker.is_some() {
            self.current_sentence = self.snapshot.sentence;
            self.current_word = self.snapshot.word;
        } else {
            self.current_sentence = self.local_sentence;
            self.current_word = self.local_word;
        }

        // Rebuild the layout when the terminal width changes.
        let new_width = usable_width(self.term_cols as usize);
        if new_width != self.layout_width {
            self.layout_width = new_width;
            self.layout = ReaderLayout::build(&self.doc, new_width);
        }

        // Smoothly follow the spoken word.
        let focus = self.layout.focus_line(self.current_sentence, self.current_word);
        let h = self.visible_height();
        let target = focus.saturating_sub(h * 2 / 5);
        self.scroll_target = target as f64;
        let diff = self.scroll_target - self.scroll;
        if diff.abs() > 0.5 {
            self.scroll += diff * (dt * 9.0).min(1.0);
        } else {
            self.scroll = self.scroll_target;
        }
        self.scroll = self
            .scroll
            .clamp(0.0, self.layout.lines.len().saturating_sub(h) as f64);
    }

    pub fn visible_height(&self) -> usize {
        (self.term_rows as usize).saturating_sub(6).max(3)
    }

    pub fn rebuild_if_needed(&mut self) {}

    pub fn toggle_theme(&mut self) {
        self.theme = self.theme.toggled();
        self.palette = Palette::of(self.theme);
        self.set_notice(format!("theme {}", self.theme.label().to_lowercase()));
    }

    pub fn toggle_play(&mut self) {
        if self.no_audio || self.speaker.is_none() {
            self.set_notice("audio is off (start without --no-play)");
            return;
        }
        if let Some(s) = &self.speaker {
            s.command(crate::tts::Command::Toggle);
        }
    }

    pub fn stop(&mut self) {
        if let Some(s) = &self.speaker {
            s.command(crate::tts::Command::Stop);
        }
    }

    pub fn next_sentence(&mut self) {
        if let Some(s) = &self.speaker {
            s.command(crate::tts::Command::Next);
        } else {
            let next = (self.local_sentence + 1).min(self.doc.sentences.len().saturating_sub(1));
            self.local_sentence = next;
            self.local_word = self.doc.sentence_first_word(next);
        }
    }

    pub fn prev_sentence(&mut self) {
        if let Some(s) = &self.speaker {
            s.command(crate::tts::Command::Prev);
        } else {
            let prev = self.local_sentence.saturating_sub(1);
            self.local_sentence = prev;
            self.local_word = self.doc.sentence_first_word(prev);
        }
    }

    pub fn speed_up(&mut self) {
        self.adjust_speed(0.1);
    }

    pub fn speed_down(&mut self) {
        self.adjust_speed(-0.1);
    }

    fn adjust_speed(&mut self, delta: f32) {
        self.speed = (self.speed + delta).clamp(0.4, 4.0);
        if let Some(s) = &self.speaker {
            s.command(crate::tts::Command::Speed(self.speed));
        }
        self.set_notice(format!("speed {:.1}x", self.speed));
    }

    pub fn skip_boot(&mut self) {
        self.boot = 1.0;
    }

    pub fn is_booting(&self) -> bool {
        self.boot < 1.0
    }
}

fn usable_width(cols: usize) -> usize {
    let outer = cols.saturating_sub(8).max(16);
    outer.min(100)
}

pub fn pulse(t: f64) -> f32 {
    (0.6 + 0.4 * (t * 2.0).sin().abs()) as f32
}
