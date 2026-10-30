mod app;
mod cli;
mod config;
mod export;
mod lang;
mod text;
mod theme;
mod tts;
mod ui;
mod util;

use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use clap::Parser;
use crate::app::App;
use crate::cli::Cli;
use crate::config::Config;
use crate::lang::Language;
use crate::text::Document;
use crate::theme::ThemeName;
use crate::tts::{SpeakerHandle, Utterance};

const FRAME: Duration = Duration::from_millis(33);

fn main() {
    if let Err(e) = run() {
        eprintln!("voices: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let (cfg, _) = crate::cli::load_config(&cli)?;

    if cli.sample_config {
        print!("{}", crate::config::sample());
        return Ok(());
    }

    // --list-voices does not need an input.
    if cli.list_voices {
        let dirs = voice_dirs(&cfg, cli.model_dir.as_deref());
        let voices = crate::tts::piper::list_voices(&dirs);
        if voices.is_empty() {
            println!("no voices found");
        } else {
            for (lang, path) in voices {
                println!("{:<4} {}", lang.short(), path.display());
            }
        }
        return Ok(());
    }

    let (input_text, source, default_title) = read_input(&cli)?;

    // Language: explicit flag, then config, then auto-detect.
    let language = cli
        .lang
        .or(cfg.language)
        .unwrap_or_else(|| Language::detect(&input_text));

    let speed = cli.speed.or(cfg.speed).unwrap_or(1.0).clamp(0.4, 4.0);
    let theme = if cli.light {
        ThemeName::Light
    } else {
        cli.theme.or(cfg.theme).unwrap_or_default()
    };

    let source_str = source.as_ref().map(|p| p.display().to_string());
    let doc = Document::parse(&input_text, source_str, &default_title);

    // Export mode: synthesise the whole document and exit.
    if let Some(output) = &cli.output {
        let piper_cfg = crate::export::resolve_config(
            language,
            cli.voice.as_deref(),
            cli.model_dir.as_deref(),
            cli.piper.as_deref(),
            &cfg,
        )?;
        crate::export::run(&doc, piper_cfg, output, speed)?;
        return Ok(());
    }

    // Resolve Piper + player unless the user asked for a preview.
    let no_audio = cli.no_play;
    let speaker = if no_audio {
        None
    } else {
        match crate::tts::piper::PiperConfig::resolve(
            language,
            cli.voice.as_deref(),
            cli.model_dir.as_deref(),
            cli.piper.as_deref(),
            &cfg,
        ) {
            Ok(piper_cfg) => {
                let player = match crate::tts::player::Player::resolve(cli.player.as_deref().or(cfg.player.as_deref())) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("voices: {e}");
                        return Ok(());
                    }
                };
                let utterances = Utterance::from_document(&doc);
                let lookahead = cli.lookahead.or(cfg.lookahead).unwrap_or(2);
                let silence = cli.sentence_silence.or(cfg.sentence_silence).unwrap_or(0.12);
                match SpeakerHandle::spawn(
                    utterances,
                    piper_cfg,
                    player,
                    language,
                    speed,
                    lookahead,
                    silence,
                ) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("voices: could not start the speaker: {e:#}");
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                eprintln!("voices: {e:#}");
                eprintln!("voices: run with --no-play to read without audio");
                return Ok(());
            }
        }
    };

    let voice_name = speaker
        .as_ref()
        .map(|s| s.snapshot().voice.clone())
        .unwrap_or_else(|| cli.voice.clone().unwrap_or_else(|| "none".to_string()));

    // Terminal setup.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let (cols, rows) = (terminal.size()?.width, terminal.size()?.height);
    let mut app = App::new(
        doc,
        theme,
        language,
        voice_name,
        speed,
        speaker,
        no_audio,
        cfg.clone(),
        cols,
        rows,
    );

    let mut last = Instant::now();
    let result = 'run: loop {
        // Drain input.
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(key) => handle_key(&mut app, key),
                Event::Resize(c, r) => {
                    app.term_cols = c;
                    app.term_rows = r;
                }
                _ => {}
            }
            if app.quit {
                break 'run;
            }
        }

        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64().min(0.1);
        last = now;
        app.update(dt);

        terminal.draw(|f| ui::draw(f, &mut app))?;

        if app.quit {
            break 'run;
        }

        let spent = last.elapsed();
        if spent < FRAME {
            std::thread::sleep(FRAME - spent);
        }
    };

    // Restore terminal.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Drop the speaker (stops audio).
    drop(app.speaker.take());

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Boot splash: any key (except q) skips it.
    if app.is_booting() {
        if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
            app.quit = true;
            return;
        }
        app.skip_boot();
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            if app.help {
                app.help = false;
            } else {
                app.quit = true;
            }
        }
        KeyCode::Char('?') | KeyCode::Char('h') => {
            app.help = !app.help;
        }
        KeyCode::Char(' ') => {
            app.toggle_play();
        }
        KeyCode::Char('s') => {
            app.stop();
        }
        KeyCode::Left => {
            app.prev_sentence();
        }
        KeyCode::Right => {
            app.next_sentence();
        }
        KeyCode::Up => {
            app.speed_up();
        }
        KeyCode::Down => {
            app.speed_down();
        }
        KeyCode::Char('t') => {
            app.toggle_theme();
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.speed_up();
        }
        KeyCode::Char('-') => {
            app.speed_down();
        }
        KeyCode::Char('r') => {
            // Retry audio on error.
            if let Some(s) = &app.speaker {
                s.command(crate::tts::Command::Retry);
            }
        }
        _ => {}
    }
}

fn read_input(cli: &Cli) -> anyhow::Result<(String, Option<PathBuf>, String)> {
    if let Some(text) = &cli.text {
        return Ok((text.clone(), None, "UNTITLED".to_string()));
    }
    if let Some(input) = &cli.input {
        let path = PathBuf::from(input);
        if input == "-" {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            return Ok((buf, None, "STDIN".to_string()));
        }
        if path.is_file() {
            let bytes = std::fs::read(&path)?;
            let text = String::from_utf8_lossy(&bytes).to_string();
            let text = text.trim_start_matches('\u{feff}').to_string();
            let title = crate::util::file_label(&path);
            return Ok((text, Some(path), title));
        }
        // A path-like argument that doesn't exist is an error, not raw text.
        if looks_like_path(input) {
            anyhow::bail!("input file not found: {input}");
        }
        return Ok((input.clone(), None, "UNTITLED".to_string()));
    }
    if !io::stdin().is_terminal() || cli.stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        if buf.trim().is_empty() {
            anyhow::bail!("no input: pass a file, --text, or pipe text on stdin");
        }
        return Ok((buf, None, "STDIN".to_string()));
    }
    anyhow::bail!("no input: pass a file path, --text \"...\", or pipe text on stdin")
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.contains('\\') || s.ends_with(".txt") || s.ends_with(".md")
}

fn voice_dirs(cfg: &Config, extra: Option<&std::path::Path>) -> Vec<PathBuf> {
    // Reuse PiperConfig's resolution by calling resolve with a dummy language
    // is not ideal; instead duplicate the small search list here.
    let mut dirs = Vec::new();
    if let Some(d) = extra {
        dirs.push(crate::util::expand_tilde(d));
    }
    if let Some(d) = &cfg.model_dir {
        dirs.push(crate::util::expand_tilde(d));
    }
    if let Some(d) = std::env::var_os("PIPER_MODEL_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(home) = crate::util::data_home() {
        dirs.push(home.join("piper"));
        dirs.push(home.join("piper").join("voices"));
        dirs.push(home.join("voices"));
    }
    dirs.push(PathBuf::from("models"));
    dirs.push(PathBuf::from("voices"));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}
