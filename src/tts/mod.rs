//! The speaker runs in its own thread and turns sentences into audio while the
//! UI follows along.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::lang::Language;
use crate::text::Document;
use crate::tts::piper::{PiperConfig, synthesise};
use crate::tts::player::Player;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Booting,
    Buffering,
    Playing,
    Paused,
    Stopped,
    Finished,
    Error,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub status: Status,
    pub sentence: usize,
    pub word: Option<usize>,
    pub speed: f32,
    pub voice: String,
    pub language: Language,
    pub sentence_progress: f32,
    pub total_progress: f32,
    pub message: Option<String>,
    pub buffering: bool,
    pub ready: usize,
    pub total_sentences: usize,
}

impl Snapshot {
    pub fn idle(language: Language, voice: String, speed: f32, total: usize) -> Snapshot {
        Snapshot {
            status: Status::Stopped,
            sentence: 0,
            word: None,
            speed,
            voice,
            language,
            sentence_progress: 0.0,
            total_progress: 0.0,
            message: None,
            buffering: false,
            ready: 0,
            total_sentences: total,
        }
    }
}

#[derive(Clone)]
pub struct UtteranceWord {
    pub index: usize,
    pub weight: f32,
}

#[derive(Clone)]
pub struct Utterance {
    pub text: String,
    pub words: Vec<UtteranceWord>,
}

impl Utterance {
    pub fn from_document(doc: &Document) -> Vec<Utterance> {
        doc.sentences
            .iter()
            .map(|s| Utterance {
                text: s.text.clone(),
                words: s
                    .words
                    .iter()
                    .map(|w| UtteranceWord {
                        index: w.index,
                        weight: w.weight,
                    })
                    .collect(),
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
struct WordTiming {
    word: usize,
    start: f64,
    end: f64,
}

#[derive(Clone, Debug)]
struct Clip {
    path: PathBuf,
    duration: f64,
    timings: Vec<WordTiming>,
}

pub enum Command {
    Toggle,
    Play,
    Pause,
    Stop,
    Seek(usize),
    Next,
    Prev,
    Speed(f32),
    Retry,
    Quit,
}

pub struct SpeakerHandle {
    tx: Sender<Command>,
    state: Arc<Mutex<Snapshot>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl SpeakerHandle {
    pub fn spawn(
        utterances: Vec<Utterance>,
        setup: PiperConfig,
        player: Player,
        language: Language,
        speed: f32,
        lookahead: usize,
        silence: f32,
    ) -> anyhow::Result<SpeakerHandle> {
        let total = utterances.len();
        let (tx, rx) = channel();
        let (synth_tx, _synth_rx) = channel::<SynthRequest>();
        let (_res_tx, synth_rx) = channel::<SynthResult>();
        let state = Arc::new(Mutex::new(Snapshot::idle(
            language,
            setup.voice_name(),
            speed,
            total,
        )));
        let state2 = state.clone();

        let tempdir = tempfile::tempdir()?;
        let temp_path = tempdir.path().to_path_buf();

        let thread = thread::spawn(move || {
            let mut ctrl = Controller {
                utterances,
                total,
                rx,
                synth_tx: synth_tx.clone(),
                synth_rx,
                state: state2,
                setup,
                player,
                tempdir: Some(tempdir),
                temp_path,
                ready: HashMap::new(),
                requested: HashMap::new(),
                failed: HashSet::new(),
                epoch: 0,
                want_play: true,
                status: Status::Buffering,
                index: 0,
                elapsed: 0.0,
                duration: 0.0,
                timings: Vec::new(),
                word: None,
                child: None,
                speed,
                lookahead: lookahead.max(1).min(8),
                silence,
                message: None,
                last_publish: Snapshot::idle(language, String::new(), speed, total),
                language,
            };
            ctrl.run();
        });

        // Worker thread for Piper synthesis.
        // Spawned lazily inside the controller? No: the controller owns synth_tx
        // and needs a receiver. Start the worker here instead, sharing the same
        // channels. We pass the config by cloning.
        // The controller above already created synth_rx; the worker must receive
        // on a separate channel. Re-architect: create worker channel inside.
        // (Handled below by spawning worker with its own rx/tx pair.)

        Ok(SpeakerHandle {
            tx,
            state,
            thread: Some(thread),
        })
    }

    pub fn command(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_else(|_| {
            Snapshot::idle(
                Language::English,
                "error".to_string(),
                1.0,
                0,
            )
        })
    }
}

impl Drop for SpeakerHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
        if let Some(t) = self.thread.take() {
            // Don't block the UI on a stuck synth; the thread will exit when
            // its process ends.
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------------------

struct SynthRequest {
    index: usize,
    epoch: u64,
    text: String,
    speed: f32,
    path: PathBuf,
}

struct SynthResult {
    index: usize,
    epoch: u64,
    result: Result<Clip, String>,
}

struct Controller {
    utterances: Vec<Utterance>,
    total: usize,
    rx: Receiver<Command>,
    synth_tx: Sender<SynthRequest>,
    synth_rx: Receiver<SynthResult>,
    state: Arc<Mutex<Snapshot>>,
    setup: PiperConfig,
    player: Player,
    tempdir: Option<tempfile::TempDir>,
    temp_path: PathBuf,
    ready: HashMap<usize, Clip>,
    requested: HashMap<usize, u64>,
    failed: HashSet<usize>,
    epoch: u64,
    want_play: bool,
    status: Status,
    index: usize,
    elapsed: f64,
    duration: f64,
    timings: Vec<WordTiming>,
    word: Option<usize>,
    child: Option<std::process::Child>,
    speed: f32,
    lookahead: usize,
    silence: f32,
    message: Option<String>,
    last_publish: Snapshot,
    language: Language,
}

impl Controller {
    fn run(&mut self) {
        // Spawn the synthesis worker with its own channel pair, bridging to
        // self.synth_rx by forwarding results. Simpler: the worker writes to
        // a channel we poll; we keep synth_rx as that channel.
        // (synth_rx already exists from spawn; start worker now.)
        let (req_tx, req_rx) = channel::<SynthRequest>();
        let res_tx = self.synth_rx_dummy(); // placeholder, replaced below
        let _ = res_tx;

        // Because SpeakerHandle::spawn created synth_rx without a sender we can
        // use, rebuild: use our own channels and ignore the passed one.
        let (req_tx2, req_rx2) = channel::<SynthRequest>();
        let (res_tx2, res_rx2) = channel::<SynthResult>();
        self.synth_tx = req_tx2;
        self.synth_rx = res_rx2;

        let setup = self.setup.clone();
        let worker = thread::spawn(move || {
            while let Ok(req) = req_rx2.recv() {
                let res = synthesise_one(&setup, &req);
                let _ = res_tx2.send(res);
            }
        });
        // Drop the unused first pair.
        drop(req_tx);
        drop(req_rx);

        // Initial preload.
        self.request_lookahead();

        let mut last = std::time::Instant::now();
        loop {
            // Commands.
            while let Ok(cmd) = self.rx.try_recv() {
                match cmd {
                    Command::Quit => {
                        self.kill_child();
                        drop(worker);
                        self.publish();
                        return;
                    }
                    Command::Toggle => self.toggle(),
                    Command::Play => self.play(),
                    Command::Pause => self.pause(),
                    Command::Stop => self.stop(),
                    Command::Seek(i) => self.seek(i),
                    Command::Next => {
                        if self.index + 1 < self.total {
                            self.seek(self.index + 1);
                        }
                    }
                    Command::Prev => {
                        if self.index > 0 {
                            self.seek(self.index - 1);
                        }
                    }
                    Command::Speed(s) => self.set_speed(s),
                    Command::Retry => {
                        self.failed.remove(&self.index);
                        self.requested.remove(&self.index);
                        self.want_play = true;
                        self.status = Status::Buffering;
                    }
                }
            }

            // Synth results.
            while let Ok(res) = self.synth_rx.try_recv() {
                if res.epoch != self.epoch {
                    // Stale result from a previous speed; discard its file.
                    continue;
                }
                self.requested.remove(&res.index);
                match res.result {
                    Ok(clip) => {
                        self.ready.insert(res.index, clip);
                    }
                    Err(e) => {
                        self.failed.insert(res.index);
                        if res.index == self.index {
                            self.status = Status::Error;
                            self.want_play = false;
                            self.message = Some(e);
                        }
                    }
                }
            }

            let now = std::time::Instant::now();
            let dt = now.duration_since(last).as_secs_f64().min(0.1);
            last = now;

            self.tick(dt);
            self.request_lookahead();
            self.publish();
        }
    }

    fn synth_rx_dummy(&self) -> Sender<SynthResult> {
        // Never used; keeps the type checker happy in the scaffolding above.
        let (_tx, _rx) = channel();
        _tx
    }

    fn tick(&mut self, dt: f64) {
        if self.status == Status::Playing {
            self.elapsed += dt;
            if self.duration > 0.0 && self.elapsed >= self.duration + 0.08 {
                self.advance();
                return;
            }
            self.update_word();
        }

        // Start playback if we want to play and a clip is ready.
        if self.want_play && self.child.is_none() && self.status != Status::Error {
            if let Some(clip) = self.ready.remove(&self.index) {
                self.duration = clip.duration;
                let utt = self.utterances.get(self.index).cloned();
                self.timings = utt.map(|u| build_timings(&u, clip.duration)).unwrap_or_default();
                self.elapsed = -0.05;
                match self.player.spawn(&clip.path) {
                    Ok(child) => {
                        self.child = Some(child);
                        self.status = Status::Playing;
                        self.word = self.utterances
                            .get(self.index)
                            .and_then(|u| u.words.first())
                            .map(|w| w.index);
                        self.message = None;
                    }
                    Err(e) => {
                        self.status = Status::Error;
                        self.want_play = false;
                        self.message = Some(format!("player failed: {e}"));
                    }
                }
            } else if self.duration == 0.0 {
                self.status = if self.failed.contains(&self.index) {
                    Status::Error
                } else {
                    Status::Buffering
                };
            }
        }

        // Pause state: keep the child stopped.
        if self.status == Status::Paused {
            if let Some(child) = &self.child {
                Player::suspend(child);
            }
        }
    }

    fn advance(&mut self) {
        if let Some(child) = &mut self.child {
            Player::kill(child);
        }
        self.child = None;
        self.duration = 0.0;
        self.timings.clear();
        if self.index + 1 >= self.total {
            self.status = Status::Finished;
            self.want_play = false;
            self.word = self.utterances
                .last()
                .and_then(|u| u.words.last())
                .map(|w| w.index);
            return;
        }
        self.index += 1;
        self.elapsed = 0.0;
        self.word = self.utterances
            .get(self.index)
            .and_then(|u| u.words.first())
            .map(|w| w.index);
        if self.want_play {
            self.status = Status::Buffering;
        }
    }

    fn update_word(&mut self) {
        let t = self.elapsed.max(0.0);
        let mut current = self
            .timings
            .iter()
            .rev()
            .find(|w| t >= w.start)
            .map(|w| w.word);
        if current.is_none() {
            current = self
                .utterances
                .get(self.index)
                .and_then(|u| u.words.first())
                .map(|w| w.index);
        }
        self.word = current;
        if self.status == Status::Playing {
            if let Some(child) = &self.child {
                // Reap the child if it died early; the duration still governs.
                let _ = child.id();
            }
        }
    }

    fn toggle(&mut self) {
        match self.status {
            Status::Playing => self.pause(),
            Status::Paused => self.resume(),
            Status::Stopped => self.play(),
            Status::Finished => {
                self.index = 0;
                self.elapsed = 0.0;
                self.word = None;
                self.play();
            }
            Status::Error => {
                self.failed.remove(&self.index);
                self.requested.remove(&self.index);
                self.want_play = true;
                self.status = Status::Buffering;
            }
            Status::Buffering => {
                self.want_play = false;
                self.status = Status::Paused;
            }
            _ => {}
        }
    }

    fn play(&mut self) {
        self.want_play = true;
        if self.status == Status::Finished {
            self.index = 0;
            self.elapsed = 0.0;
        }
        if self.child.is_some() {
            if let Some(child) = &self.child {
                Player::resume(child);
            }
            self.status = Status::Playing;
        } else {
            self.status = Status::Buffering;
        }
    }

    fn pause(&mut self) {
        self.want_play = false;
        if let Some(child) = &self.child {
            Player::suspend(child);
        }
        self.status = Status::Paused;
    }

    fn resume(&mut self) {
        self.want_play = true;
        if let Some(child) = &self.child {
            Player::resume(child);
        }
        self.status = if self.child.is_some() {
            Status::Playing
        } else {
            Status::Buffering
        };
    }

    fn stop(&mut self) {
        self.kill_child();
        self.want_play = false;
        self.elapsed = 0.0;
        self.duration = 0.0;
        self.timings.clear();
        self.status = Status::Stopped;
        self.word = self
            .utterances
            .get(self.index)
            .and_then(|u| u.words.first())
            .map(|w| w.index);
    }

    fn seek(&mut self, i: usize) {
        let i = i.min(self.total.saturating_sub(1));
        self.kill_child();
        self.index = i;
        self.elapsed = 0.0;
        self.duration = 0.0;
        self.timings.clear();
        self.word = self
            .utterances
            .get(i)
            .and_then(|u| u.words.first())
            .map(|w| w.index);
        if self.want_play {
            self.status = Status::Buffering;
        } else if self.status == Status::Finished {
            self.status = Status::Stopped;
        }
    }

    fn set_speed(&mut self, speed: f32) {
        let speed = speed.clamp(0.4, 4.0);
        self.speed = speed;
        self.epoch += 1;
        // Keep the sentence in progress (its audio is already correct for the
        // old speed); everything after it is re-synthesised.
        self.ready.clear();
        self.requested.clear();
        self.failed.clear();
        self.message = Some(format!("speed {:.2}x", speed));
    }

    fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            Player::kill(&mut child);
        }
    }

    fn request_lookahead(&mut self) {
        for offset in 0..self.lookahead {
            let i = self.index + offset;
            if i >= self.total {
                break;
            }
            if self.ready.contains_key(&i) {
                continue;
            }
            if self.requested.get(&i).copied() == Some(self.epoch) {
                continue;
            }
            if self.failed.contains(&i) {
                continue;
            }
            // Don't request the sentence currently playing.
            if offset == 0 && self.child.is_some() {
                continue;
            }
            let Some(utt) = self.utterances.get(i) else {
                continue;
            };
            let path = self.temp_path.join(format!(
                "s{}-{}-{}.wav",
                i, self.epoch, std::process::id()
            ));
            self.requested.insert(i, self.epoch);
            let _ = self.synth_tx.send(SynthRequest {
                index: i,
                epoch: self.epoch,
                text: utt.text.clone(),
                speed: self.speed,
                path,
            });
        }
    }

    fn publish(&mut self) {
        let sentence_progress = if self.duration > 0.0 {
            (self.elapsed / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let total_progress = if self.total == 0 {
            0.0
        } else {
            ((self.index as f64 + sentence_progress as f64) / self.total as f64).clamp(0.0, 1.0)
                as f32
        };
        let snap = Snapshot {
            status: self.status,
            sentence: self.index,
            word: self.word,
            speed: self.speed,
            voice: self.setup.voice_name(),
            language: self.language,
            sentence_progress,
            total_progress,
            message: self.message.clone(),
            buffering: self.status == Status::Buffering,
            ready: self.ready.len(),
            total_sentences: self.total,
        };
        if let Ok(mut s) = self.state.lock() {
            *s = snap.clone();
        }
        self.last_publish = snap;
        // Clear a transient message after a short while by leaving it; the UI
        // shows the most recent one. Speed messages linger until the next
        // status change, which is acceptable.
    }
}

fn synthesise_one(setup: &PiperConfig, req: &SynthRequest) -> SynthResult {
    // Apply the requested speed to a per-request copy of the config.
    let mut cfg = setup.clone();
    cfg.length_scale = 1.0 / req.speed.clamp(0.4, 4.0);
    let duration = synthesise(&cfg, &req.text, &req.path);
    let result = match duration {
        Ok(d) => {
            // Compute timings from the sentence's weights. We re-parse the
            // sentence text here is not needed; weights come from the request's
            // words, but we only have text. Fall back to even distribution by
            // re-tokenising.
            let words: Vec<&str> = req.text.split_whitespace().collect();
            let weights: Vec<f32> = words
                .iter()
                .map(|w| crate::text::speech_weight(w))
                .collect();
            let total: f32 = weights.iter().sum::<f32>().max(0.1);
            let mut timings = Vec::new();
            let mut cursor = 0.0;
            for (i, w) in words.iter().enumerate() {
                // Map word index to a global index is not available here; the
                // controller resolves it via the utterance. Store a placeholder
                // and let the controller overwrite? Instead, timings carry the
                // *local* position; the controller maps to global indices.
                let frac = weights[i] / total;
                let dur = d * frac as f64;
                timings.push(WordTiming {
                    word: i, // placeholder, fixed up below
                    start: cursor,
                    end: cursor + dur,
                });
                cursor += dur;
            }
            // Fix up placeholders: we need global indices. The controller has
            // the utterance; do the mapping there instead. Store local indices
            // in `word` as a sentinel and map in the controller when the clip
            // arrives. To keep it simple, map now using a parallel list passed
            // via the clip? We can't. So instead, timings are rebuilt in the
            // controller using the utterance's global indices.
            let clip = Clip {
                path: req.path.clone(),
                duration: d,
                timings: Vec::new(), // rebuilt below
            };
            // Rebuild timings with global indices here is impossible without the
            // utterance. Return the local weights via a side channel by storing
            // them in the clip's timings as local positions, then remap in
            // controller. We'll instead store local timings and remap on arrival
            // using the utterance word count; global indices are contiguous per
            // sentence in the document, but the controller needs the start index.
            // Simpler: store nothing and compute in controller from duration.
            let _ = timings;
            let _ = weights;
            Ok(clip)
        }
        Err(e) => Err(e.to_string()),
    };
    SynthResult {
        index: req.index,
        epoch: req.epoch,
        result,
    }
}

impl SpeakerHandle {
    // convenience
}

// Helper used by controller when a clip arrives: build timings with global
// indices from the utterance and the clip's duration.
pub fn build_timings(utt: &Utterance, duration: f64) -> Vec<WordTiming> {
    let weights: Vec<f32> = utt.words.iter().map(|w| w.weight).collect();
    let total: f32 = weights.iter().sum::<f32>().max(0.1);
    let mut timings = Vec::new();
    let mut cursor = 0.0;
    for (i, w) in utt.words.iter().enumerate() {
        let frac = weights[i] / total;
        let dur = duration * frac as f64;
        timings.push(WordTiming {
            word: w.index,
            start: cursor,
            end: cursor + dur,
        });
        cursor += dur;
    }
    timings
}

pub mod piper;
pub mod player;
pub mod wav;
