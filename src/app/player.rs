//! The in-app video player: a poster that plays in place, with play and pause, a position bar
//! to seek with, volume, and the keyboard. Frames and sound come from `crate::media` workers;
//! nothing here decodes, reads a file or waits.
use super::{
    preview::Previews,
    theme::{DANGER, RADIUS_M, SPACE_M, SPACE_S, SPACE_XS, TEXT_CAPTION},
};
use crate::media::{
    audio::{self, Audio},
    playback::{self, Decoder, Frame, Info, Step},
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const MAX_WIDTH: f32 = 720.;
const MAX_HEIGHT: f32 = 520.;
const MIN_HEIGHT: f32 = 160.;
/// The control strip under the picture.
const STRIP: f32 = 44.;
const BACKDROP: Color32 = Color32::BLACK;
const STRIP_FILL: Color32 = Color32::from_rgb(17, 19, 27);
/// The strip over a fullscreen picture.
const STRIP_OVER: Color32 = Color32::from_rgba_premultiplied(12, 13, 19, 215);
const CONTROL: Color32 = Color32::from_rgb(236, 238, 245);
const CONTROL_OFF: Color32 = Color32::from_rgb(120, 126, 142);
const TRACK: Color32 = Color32::from_rgb(70, 75, 92);
const PLAYED: Color32 = Color32::from_rgb(129, 140, 248);
/// While the bar is dragged, a new moment is decoded at most this often.
const SCRUB_EVERY: Duration = Duration::from_millis(120);
/// A paused video gives up its helpers after this long; playing again starts them where it was.
const PARK_AFTER: Duration = Duration::from_secs(30);
/// The arrow keys move this far.
const STEP: f64 = 5.;
/// A picture this far behind its moment means the decoder cannot keep up at this frame rate.
const BEHIND: f64 = 0.5;
/// How long a run gets before it is judged, and where a run that catches up starts.
const SETTLE: Duration = Duration::from_millis(1500);
const CATCH_UP: f64 = 0.25;
const MIN_RATE: f64 = 15.;
/// Fullscreen controls and the pointer hide this long after the last movement or key press.
const HIDE_AFTER: Duration = Duration::from_millis(2500);

/// The video's time: a wall clock that sound, while there is some, keeps correcting.
struct Clock {
    at: f64,
    since: Option<Instant>,
}

impl Clock {
    fn now(&self) -> f64 {
        self.at + self.since.map_or(0., |since| since.elapsed().as_secs_f64())
    }
    fn set(&mut self, seconds: f64) {
        self.at = seconds;
        if self.since.is_some() {
            self.since = Some(Instant::now());
        }
    }
    fn start(&mut self) {
        self.since.get_or_insert_with(Instant::now);
    }
    fn stop(&mut self) {
        self.at = self.now();
        self.since = None;
    }
}

/// One opened video.
struct Session {
    path: PathBuf,
    /// `None` once the video ended or failed, and while it is parked.
    decoder: Option<Decoder>,
    audio: Option<Audio>,
    info: Option<Info>,
    info_rx: mpsc::Receiver<Option<Info>>,
    probe_cancel: Arc<AtomicBool>,
    texture: Option<egui::TextureHandle>,
    /// Width over height of the picture, once a frame has arrived.
    aspect: Option<f32>,
    /// A decoded frame whose moment has not come.
    next: Option<Frame>,
    clock: Clock,
    /// What the person asked for; the clock runs once a frame is there to show.
    playing: bool,
    /// Since when the first frame after opening or seeking is awaited.
    waiting: Option<Instant>,
    ended: bool,
    error: Option<String>,
    /// The moment of the frame on screen.
    shown: f64,
    /// The most frames a second asked of the decoder; halved when it cannot keep up.
    rate: f64,
    /// Where the position bar is being dragged to.
    drag: Option<f64>,
    last_seek: Instant,
    paused_at: Instant,
    /// Scroll the player into view the next time it is drawn.
    reveal: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.probe_cancel.store(true, Ordering::Relaxed);
    }
}

fn wake(ctx: &egui::Context) -> impl Fn() + Send + 'static {
    let ctx = ctx.clone();
    move || ctx.request_repaint()
}

impl Session {
    fn open(ctx: &egui::Context, path: PathBuf, gain: f32) -> Self {
        let (tx, info_rx) = mpsc::channel();
        let probe_cancel = Arc::new(AtomicBool::new(false));
        let (file, cancel, repaint) = (path.clone(), probe_cancel.clone(), wake(ctx));
        std::thread::spawn(move || {
            let _ = tx.send(playback::probe(&file, &cancel));
            repaint();
        });
        let now = Instant::now();
        Self {
            decoder: Some(Decoder::start(&path, 0., playback::MAX_RATE, wake(ctx))),
            audio: Audio::start(&path, 0., gain),
            path,
            info: None,
            info_rx,
            probe_cancel,
            texture: None,
            aspect: None,
            next: None,
            clock: Clock {
                at: 0.,
                since: None,
            },
            playing: true,
            waiting: Some(now),
            ended: false,
            error: None,
            shown: 0.,
            rate: playback::MAX_RATE,
            drag: None,
            last_seek: now,
            paused_at: now,
            reveal: true,
        }
    }

    fn duration(&self) -> Option<f64> {
        self.info.and_then(|info| info.duration)
    }

    /// The moment the position bar shows.
    fn position(&self) -> f64 {
        let position = self.drag.unwrap_or_else(|| self.clock.now()).max(0.);
        self.duration().map_or(position, |end| position.min(end))
    }

    /// Starts again from `seconds`; the frame on screen stays until the new one arrives.
    fn seek(&mut self, ctx: &egui::Context, seconds: f64, sound: bool, gain: f32) {
        // Just before the end at most, so there is a frame to show.
        let last = self.duration().map_or(f64::MAX, |end| (end - 0.1).max(0.));
        let seconds = seconds.clamp(0., last);
        // The old sound stops before the new one is started.
        self.audio = None;
        if sound && self.info.is_none_or(|info| info.audio) {
            self.audio = Audio::start(&self.path, seconds, gain);
        }
        self.decoder = Some(Decoder::start(&self.path, seconds, self.rate, wake(ctx)));
        self.next = None;
        self.ended = false;
        self.error = None;
        self.clock.stop();
        self.clock.set(seconds);
        self.waiting = Some(Instant::now());
        self.last_seek = Instant::now();
    }

    fn run(&mut self) {
        self.clock.start();
        if let Some(audio) = &self.audio {
            audio.set_playing(true);
        }
    }

    fn set_playing(&mut self, ctx: &egui::Context, playing: bool, gain: f32) {
        if playing == self.playing || self.error.is_some() {
            return;
        }
        self.playing = playing;
        if !playing {
            self.clock.stop();
            self.paused_at = Instant::now();
            if let Some(audio) = &self.audio {
                audio.set_playing(false);
            }
        } else if self.ended {
            self.seek(ctx, 0., true, gain);
        } else if self.decoder.is_none() {
            self.seek(ctx, self.position(), true, gain);
        } else if self.waiting.is_none() {
            self.run();
        }
    }

    /// Stops the helpers of a paused video and keeps its picture and position.
    fn park(&mut self) {
        self.playing = false;
        self.clock.stop();
        self.drag = None;
        self.decoder = None;
        self.audio = None;
        self.next = None;
        self.waiting = None;
    }

    /// Moves along the position bar: to each moment while dragging, with sound on release.
    fn scrub(&mut self, ctx: &egui::Context, seconds: f64, released: bool, gain: f32) {
        if released {
            self.drag = None;
            self.seek(ctx, seconds, true, gain);
            return;
        }
        if self.drag.is_none() {
            self.clock.stop();
            self.audio = None;
        }
        self.drag = Some(seconds);
        if self.last_seek.elapsed() >= SCRUB_EVERY {
            self.seek(ctx, seconds, false, gain);
        }
    }

    /// The picture fell behind its moment: the decoder starts again just ahead of the clock,
    /// asked for half as many frames a second. The sound, and so the clock, plays on.
    fn catch_up(&mut self, ctx: &egui::Context, now: f64) {
        self.rate = (self.rate / 2.).max(MIN_RATE);
        self.decoder = Some(Decoder::start(
            &self.path,
            now + CATCH_UP,
            self.rate,
            wake(ctx),
        ));
        self.next = None;
        self.last_seek = Instant::now();
    }

    fn step(&mut self) -> Option<Step> {
        self.next
            .take()
            .map(Step::Frame)
            .or_else(|| self.decoder.as_ref()?.next())
    }

    fn display(&mut self, ctx: &egui::Context, frame: Frame) {
        let image =
            egui::ColorImage::from_rgba_premultiplied([frame.width, frame.height], &frame.rgba);
        self.aspect = Some(frame.width as f32 / frame.height as f32);
        self.shown = frame.seconds;
        match &mut self.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture =
                    Some(ctx.load_texture("video-player", image, egui::TextureOptions::LINEAR));
            }
        }
    }

    fn finish(&mut self, result: Result<(), String>) {
        match result {
            // A run that gave nothing after frames were shown is the end, not a broken file.
            Err(error) if self.texture.is_none() => self.error = Some(error),
            _ => {
                self.ended = true;
                let end = self.duration().unwrap_or(self.shown).max(self.shown);
                self.clock.set(end);
            }
        }
        self.park();
    }

    /// Takes what the workers have ready and shows the frame whose moment has come.
    fn advance(&mut self, ctx: &egui::Context, gain: f32) {
        if let Ok(info) = self.info_rx.try_recv() {
            self.info = info;
            if info.is_some_and(|info| !info.audio) {
                self.audio = None;
            }
        }
        if let Some(audio) = &self.audio {
            audio.set_gain(gain);
        }
        if self.waiting.is_some() {
            match self.step() {
                Some(Step::Frame(frame)) => {
                    self.clock.set(frame.seconds);
                    self.display(ctx, frame);
                    self.waiting = None;
                    if self.playing && self.drag.is_none() {
                        self.run();
                    }
                }
                Some(Step::End(result)) => self.finish(result),
                // The decoder wakes the page when the frame is there.
                None => return,
            }
        }
        if !self.playing {
            if self.decoder.is_some() && self.drag.is_none() {
                if self.paused_at.elapsed() >= PARK_AFTER {
                    self.park();
                } else {
                    ctx.request_repaint_after(PARK_AFTER);
                }
            }
            return;
        }
        if self.drag.is_some() {
            return;
        }
        let heard = self.audio.as_ref().and_then(Audio::position);
        if let Some(heard) = heard {
            self.clock.set(heard);
        }
        let now = self.clock.now();
        let mut latest = None;
        let wait = loop {
            match self.step() {
                Some(Step::Frame(frame)) if frame.seconds <= now + 0.004 => latest = Some(frame),
                Some(Step::Frame(frame)) => {
                    let wait = frame.seconds - now;
                    self.next = Some(frame);
                    break wait;
                }
                Some(Step::End(result)) => {
                    if let Some(frame) = latest.take() {
                        self.display(ctx, frame);
                    }
                    self.finish(result);
                    return;
                }
                None => break 0.01,
            }
        };
        if let Some(frame) = latest {
            self.display(ctx, frame);
        }
        // Late frames were skipped above; a decoder that is behind even so gets less to do.
        if now - self.shown > BEHIND && self.last_seek.elapsed() >= SETTLE {
            self.catch_up(ctx, now);
        }
        ctx.request_repaint_after(Duration::from_secs_f64(wait.clamp(0.002, 0.05)));
    }
}

/// The window is fullscreen for the open video.
struct Fullscreen {
    /// To leave fullscreen from where no frame is drawn, as when the player is closed.
    ctx: egui::Context,
    /// The last pointer movement or key press: the controls hide a while after it.
    active: Instant,
}

impl Fullscreen {
    /// The controls stay while the video is paused or the pointer is on them.
    fn controls_shown(&self, now: Instant, playing: bool, held: bool) -> bool {
        !playing || held || now.saturating_duration_since(self.active) < HIDE_AFTER
    }
}

/// While a player is fullscreen, the context holds the pass it was last drawn in.
#[derive(Clone, Copy)]
struct Drawn(u64);

fn drawn_id() -> egui::Id {
    egui::Id::new("player-fullscreen-drawn")
}

fn mark_drawn(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.insert_temp(drawn_id(), Drawn(pass)));
}

/// Leaves fullscreen at the end of a pass that did not draw the fullscreen player: a page that
/// stops showing it must not leave the window fullscreen with nothing to leave it by.
fn watch_fullscreen(ctx: &egui::Context) {
    let watching = egui::Id::new("player-fullscreen-watch");
    if ctx.data(|data| data.get_temp::<bool>(watching)).is_some() {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(watching, true));
    ctx.on_end_pass(
        "player-fullscreen",
        Arc::new(|ctx| {
            if ctx.viewport_id() != egui::ViewportId::ROOT {
                return;
            }
            let id = drawn_id();
            let pass = ctx.cumulative_pass_nr();
            if let Some(Drawn(drawn)) = ctx.data(|data| data.get_temp(id))
                && drawn < pass
            {
                ctx.data_mut(|data| data.remove::<Drawn>(id));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }),
    );
}

pub struct Player {
    session: Option<Session>,
    volume: f32,
    muted: bool,
    /// A failure to open the video in the system player.
    error: Option<String>,
    fullscreen: Option<Fullscreen>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            session: None,
            volume: 1.,
            muted: false,
            error: None,
            fullscreen: None,
        }
    }
}

impl Player {
    fn gain(&self) -> f32 {
        if self.muted {
            0.
        } else {
            // Closer to how loud it sounds than the plain factor.
            self.volume * self.volume
        }
    }

    /// Starts playing `path` from its beginning, in place of whatever was open.
    pub fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.leave_fullscreen();
        self.error = None;
        // The previous video's helpers stop before the new ones start.
        self.session = None;
        self.session = Some(Session::open(ctx, path, self.gain()));
    }

    pub fn close(&mut self) {
        self.leave_fullscreen();
        self.session = None;
        self.error = None;
    }

    /// The page is not shown: pauses and stops the helpers, keeping the picture and position.
    pub fn suspend(&mut self) {
        self.leave_fullscreen();
        if let Some(session) = &mut self.session
            && (session.playing || session.decoder.is_some())
        {
            session.park();
        }
    }

    /// The video that is open, playing or not.
    pub fn current(&self) -> Option<&Path> {
        self.session.as_ref().map(|session| session.path.as_path())
    }

    pub fn is_open(&self, path: &Path) -> bool {
        self.current() == Some(path)
    }

    /// Makes the window fullscreen for the open video, or restores it as it was. The window
    /// keeps its own placement, so it comes back maximized or at its size.
    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        if !on {
            self.leave_fullscreen();
            return;
        }
        if self.fullscreen.is_some() || self.session.is_none() {
            return;
        }
        // The page underneath must not take the keys meant for the player.
        ctx.memory_mut(|memory| memory.stop_text_input());
        mark_drawn(ctx);
        watch_fullscreen(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        self.fullscreen = Some(Fullscreen {
            ctx: ctx.clone(),
            active: Instant::now(),
        });
    }

    fn leave_fullscreen(&mut self) {
        if let Some(fullscreen) = self.fullscreen.take() {
            let ctx = fullscreen.ctx;
            ctx.data_mut(|data| data.remove::<Drawn>(drawn_id()));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// Hands the video to the system's player: this one pauses, so the two do not play at once,
    /// and the window leaves fullscreen so the other player is not behind it.
    fn open_externally(
        &mut self,
        ctx: &egui::Context,
        open: impl FnOnce(&Path) -> Result<(), String>,
    ) {
        self.leave_fullscreen();
        let gain = self.gain();
        let Some(session) = &mut self.session else {
            return;
        };
        session.set_playing(ctx, false, gain);
        self.error = open(&session.path).err();
    }

    /// The player for `path`: its poster until it is clicked, then the video. A video open for
    /// another file is closed.
    pub fn show(&mut self, ui: &mut egui::Ui, previews: &mut Previews, path: &Path) {
        if self.session.as_ref().is_some_and(|s| s.path != path) {
            self.close();
        }
        let width = ui.available_width().min(MAX_WIDTH);
        let poster = Vec2::new(width, width * 9. / 16.);
        let height_id = ui.id().with("player-height");
        if self.session.is_none() {
            let response = thumbnail(ui, previews, path, poster)
                .on_hover_text("Play video")
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            // The box starts from the poster's shape when the video opens.
            ui.ctx().animate_value_with_time(height_id, poster.y, 0.);
            if response.clicked() {
                self.open(ui.ctx(), path.to_owned());
            }
        } else {
            self.video(ui, previews, width, poster.y, height_id);
        }
        if let Some(error) = &self.error {
            ui.colored_label(DANGER, error);
        }
    }

    fn video(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        width: f32,
        poster_height: f32,
        height_id: egui::Id,
    ) {
        let ctx = ui.ctx().clone();
        let gain = self.gain();
        let Some(session) = &mut self.session else {
            return;
        };
        session.advance(&ctx, gain);
        // A tall picture still leaves the strip inside the part of the page that is visible.
        let room = (ui.clip_rect().height() - STRIP - 2. * SPACE_M).max(MIN_HEIGHT);
        let target = session.aspect.map_or(poster_height, |aspect| {
            (width / aspect).max(width * 0.3).min(MAX_HEIGHT.min(room))
        });
        let height = ctx.animate_value_with_time(height_id, target, 0.16);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(width, height + STRIP), egui::Sense::hover());
        let id = ui.id().with("player");

        if self.fullscreen.is_some()
            && ctx
                .data(|data| data.get_temp::<Drawn>(drawn_id()))
                .is_none()
        {
            // The window already left fullscreen without this player.
            self.fullscreen = None;
        }
        if self.fullscreen.is_some() {
            mark_drawn(&ctx);
            // The page keeps the player's place, so it is as it was when fullscreen ends.
            ui.painter().rect_filled(rect, RADIUS_M, BACKDROP);
            let screen = ctx.content_rect();
            egui::Area::new(id.with("fullscreen-layer"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .constrain(false)
                .show(&ctx, |ui| {
                    ui.set_clip_rect(screen);
                    ui.allocate_rect(screen, egui::Sense::hover());
                    self.surface(ui, previews, id, screen, None);
                });
            return;
        }

        // Brought into view when opened, and kept there while the box takes the picture's shape.
        if session.reveal {
            ui.scroll_to_rect(rect, None);
            session.reveal = session.aspect.is_none() || height != target;
        }
        let picture = Rect::from_min_size(rect.min, Vec2::new(width, height));
        let strip = Rect::from_min_max(picture.left_bottom(), rect.max);
        self.surface(ui, previews, id, picture, Some(strip));
    }

    /// The picture and its controls: on the page with the strip under the picture, or
    /// fullscreen (`strip` is `None`) with the strip over the bottom of the picture while it is
    /// not hidden.
    fn surface(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        id: egui::Id,
        picture: Rect,
        strip: Option<Rect>,
    ) {
        let ctx = ui.ctx().clone();
        let gain = self.gain();
        let Some(session) = &mut self.session else {
            return;
        };
        let fullscreen = strip.is_none();
        let top = egui::CornerRadius {
            nw: RADIUS_M,
            ne: RADIUS_M,
            sw: 0,
            se: 0,
        };
        let bottom = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: RADIUS_M,
            se: RADIUS_M,
        };
        let (top, bottom) = if fullscreen {
            (egui::CornerRadius::ZERO, egui::CornerRadius::ZERO)
        } else {
            (top, bottom)
        };
        ui.painter().rect_filled(picture, top, BACKDROP);

        // The picture, or the poster until the first frame is there.
        let (image, aspect) = match (&session.texture, session.aspect) {
            (Some(texture), Some(aspect)) => (Some(texture.id()), aspect),
            _ => (previews.cached(&session.path), 16. / 9.),
        };
        if let Some(image) = image {
            let fitted = fit(picture, aspect);
            let corners = if fitted.width() >= picture.width() - 1. {
                top
            } else {
                egui::CornerRadius::ZERO
            };
            egui::Image::from_texture((image, fitted.size()))
                .corner_radius(corners)
                .paint_at(ui, fitted);
        }

        // Fullscreen, the strip lies over the bottom of the picture and hides with the pointer
        // a while after the last movement or key press.
        let over = Rect::from_min_max(
            Pos2::new(picture.left(), picture.bottom() - STRIP),
            picture.max,
        );
        let mut shown = true;
        if let Some(state) = &mut self.fullscreen {
            let now = Instant::now();
            if ctx.input(stirred) {
                state.active = now;
            }
            let held = session.drag.is_some() || ui.rect_contains_pointer(over);
            shown = state.controls_shown(now, session.playing, held);
            if shown && session.playing && !held {
                let left = HIDE_AFTER.saturating_sub(now.saturating_duration_since(state.active));
                ctx.request_repaint_after(left + Duration::from_millis(20));
            }
        }

        let surface = ui.interact(picture, id.with("picture"), egui::Sense::click());
        let surface = if fullscreen {
            surface
        } else {
            surface.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        let mut toggle = surface.clicked();
        // Both clicks of a double click toggle playing, so it plays on as it was.
        let mut toggle_fullscreen = surface.double_clicked();
        if let Some(error) = &session.error {
            let text = ui.painter().layout(
                error.clone(),
                egui::FontId::proportional(TEXT_CAPTION + 1.),
                CONTROL,
                picture.width() - 2. * SPACE_M,
            );
            let corner = picture.center() - text.size() / 2.;
            ui.painter().galley(corner, text, CONTROL);
        } else if session.waiting.is_some_and(|since| {
            since.elapsed() > Duration::from_millis(350) && session.drag.is_none()
        }) {
            egui::Spinner::new().color(CONTROL).paint_at(
                ui,
                Rect::from_center_size(picture.center(), Vec2::splat(32.)),
            );
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if session.waiting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if !session.playing && session.drag.is_none() {
            play_badge(ui, picture.center(), 64., surface.hovered());
        }

        let silent_file = session.info.is_some_and(|info| !info.audio);
        let sound = audio::AVAILABLE && !silent_file;
        let mut external = false;
        if let Some(strip) = strip.or(shown.then_some(over)) {
            ui.painter().rect_filled(
                strip,
                bottom,
                if fullscreen { STRIP_OVER } else { STRIP_FILL },
            );
            // The strip, from both ends towards the position bar in the middle.
            let middle = strip.center().y;
            let button = |x: f32| Rect::from_center_size(Pos2::new(x, middle), Vec2::splat(32.));
            let mut left = strip.left() + SPACE_S;
            let mut right = strip.right() - SPACE_S;

            let play = control(ui, id.with("play"), button(left + 16.));
            let play = play.on_hover_text(if session.playing { "Pause" } else { "Play" });
            if session.playing {
                paint_pause(ui, play.rect.center(), CONTROL);
            } else {
                paint_play(ui, play.rect.center(), 7., CONTROL);
            }
            toggle |= play.clicked();
            left += 32. + SPACE_S;

            let font = egui::FontId::proportional(TEXT_CAPTION + 1.);
            let position = session.position();
            let time = match session.duration() {
                Some(end) => format!("{} / {}", clock_text(position), clock_text(end)),
                None => clock_text(position),
            };
            // As wide as it gets, so the bar does not move while the time counts.
            let widest: String = time
                .chars()
                .map(|c| if c.is_ascii_digit() { '0' } else { c })
                .collect();
            let reserved = ui
                .painter()
                .layout_no_wrap(widest, font.clone(), CONTROL)
                .size()
                .x;
            ui.painter().text(
                Pos2::new(left, middle),
                egui::Align2::LEFT_CENTER,
                time,
                font,
                CONTROL,
            );
            left += reserved + SPACE_M;

            let full = control(ui, id.with("fullscreen"), button(right - 16.)).on_hover_text(
                if fullscreen {
                    "Exit fullscreen"
                } else {
                    "Fullscreen"
                },
            );
            paint_fullscreen(ui, full.rect.center(), CONTROL, fullscreen);
            toggle_fullscreen |= full.clicked();
            right -= 32. + SPACE_XS;

            let open = control(ui, id.with("external"), button(right - 16.))
                .on_hover_text("Open in your video player");
            paint_external(ui, open.rect.center(), CONTROL);
            external = open.clicked();
            right -= 32. + SPACE_XS;

            // Sound: a level to drag and a speaker to mute with, where there is sound to control.
            let level = Rect::from_min_max(
                Pos2::new(right - 56., middle - 12.),
                Pos2::new(right, middle + 12.),
            );
            if sound {
                let response = ui
                    .interact(level, id.with("volume"), egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if let Some(pointer) = response.interact_pointer_pos() {
                    self.volume = ((pointer.x - level.left()) / level.width()).clamp(0., 1.);
                    self.muted = false;
                }
                let shown = if self.muted { 0. } else { self.volume };
                paint_bar(ui, level, shown, response.hovered() || response.dragged());
                right -= level.width() + SPACE_XS;
            }
            let speaker = control(ui, id.with("mute"), button(right - 16.));
            let speaker = speaker.on_hover_text(if silent_file {
                "This video has no sound"
            } else if !audio::AVAILABLE {
                "Sound does not play in the app on this system yet. Open the video in your player to hear it."
            } else if self.muted {
                "Unmute"
            } else {
                "Mute"
            });
            let quiet = self.muted || self.volume == 0.;
            paint_speaker(
                ui,
                speaker.rect.center(),
                if sound { CONTROL } else { CONTROL_OFF },
                !sound || quiet,
            );
            if sound && speaker.clicked() {
                self.muted = !quiet;
                if quiet && self.volume == 0. {
                    self.volume = 0.5;
                }
            }
            right -= 32. + SPACE_M;

            let bar = Rect::from_min_max(
                Pos2::new(left, middle - 12.),
                Pos2::new(right.max(left + 1.), middle + 12.),
            );
            let fraction = session.duration().map_or(0., |end| (position / end) as f32);
            if let Some(end) = session.duration() {
                let response = ui
                    .interact(bar, id.with("seek"), egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let at = |x: f32| f64::from(((x - bar.left()) / bar.width()).clamp(0., 1.)) * end;
                let pointer = response.interact_pointer_pos().map(|pointer| at(pointer.x));
                if response.clicked() || response.drag_stopped() {
                    if let Some(seconds) = pointer.or(session.drag) {
                        session.scrub(&ctx, seconds, true, gain);
                    }
                } else if response.dragged()
                    && let Some(seconds) = pointer
                {
                    session.scrub(&ctx, seconds, false, gain);
                    ctx.request_repaint_after(SCRUB_EVERY);
                }
                let lit = response.hovered() || response.dragged();
                if let Some(hover) = response.hover_pos().filter(|_| lit) {
                    response.on_hover_text_at_pointer(clock_text(at(hover.x)));
                }
                paint_bar(ui, bar, fraction, lit);
            } else {
                paint_bar(ui, bar, fraction, false);
            }
        }

        // The keyboard: fullscreen always, on the page while the pointer is over the player and
        // nothing is being typed.
        let area = strip.map_or(picture, |strip| picture.union(strip));
        let mut jump = 0.;
        let mut leave = false;
        if fullscreen || (ui.rect_contains_pointer(area) && !ctx.wants_keyboard_input()) {
            ui.input_mut(|input| {
                let mut pressed = |key: egui::Key| input.consume_key(egui::Modifiers::NONE, key);
                toggle |= pressed(egui::Key::Space) | pressed(egui::Key::K);
                if pressed(egui::Key::ArrowLeft) {
                    jump -= STEP;
                }
                if pressed(egui::Key::ArrowRight) {
                    jump += STEP;
                }
                if sound && pressed(egui::Key::M) {
                    self.muted = !self.muted;
                }
                toggle_fullscreen |= pressed(egui::Key::F);
                // Escape only leaves fullscreen; nothing else on the page sees it.
                leave = fullscreen && pressed(egui::Key::Escape);
            });
        }
        if fullscreen && !shown {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
        if toggle {
            session.set_playing(&ctx, !session.playing, gain);
        }
        if jump != 0. && session.error.is_none() {
            session.seek(&ctx, session.position() + jump, true, gain);
        }
        if external {
            self.open_externally(&ctx, crate::platform::open_file);
        } else if leave {
            self.leave_fullscreen();
        } else if toggle_fullscreen {
            self.set_fullscreen(&ctx, !fullscreen);
        }
    }
}

/// A pointer movement, a click, a scroll or a key press: shows the hidden fullscreen controls.
fn stirred(input: &egui::InputState) -> bool {
    input.pointer.delta() != Vec2::ZERO
        || input.pointer.any_pressed()
        || input.raw_scroll_delta != Vec2::ZERO
        || input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
}

/// A video's poster with a play mark, for lists and for the player before it starts.
pub fn thumbnail(
    ui: &mut egui::Ui,
    previews: &mut Previews,
    path: &Path,
    size: Vec2,
) -> egui::Response {
    // A poster is where playing starts: have the sound output ready by then.
    audio::warm_up();
    let response = previews.show(ui, path, None, size);
    if ui.is_rect_visible(response.rect) {
        let diameter = (size.y * 0.42).clamp(22., 64.);
        play_badge(ui, response.rect.center(), diameter, response.hovered());
    }
    response
}

/// The largest rectangle of `aspect` (width over height) centered in `area`.
fn fit(area: Rect, aspect: f32) -> Rect {
    let size = if area.width() / area.height() > aspect {
        Vec2::new(area.height() * aspect, area.height())
    } else {
        Vec2::new(area.width(), area.width() / aspect)
    };
    Rect::from_center_size(area.center(), size)
}

/// `1:05`, or `1:01:05` from an hour on.
fn clock_text(seconds: f64) -> String {
    let total = seconds.max(0.) as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// A control on the strip: a click area that lights up under the pointer.
fn control(ui: &mut egui::Ui, id: egui::Id, rect: Rect) -> egui::Response {
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, RADIUS_M, Color32::from_white_alpha(28));
    }
    response
}

fn paint_play(ui: &egui::Ui, center: Pos2, size: f32, color: Color32) {
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + Vec2::new(-size * 0.7, -size),
            center + Vec2::new(size, 0.),
            center + Vec2::new(-size * 0.7, size),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

fn paint_pause(ui: &egui::Ui, center: Pos2, color: Color32) {
    for x in [-4., 4.] {
        ui.painter().rect_filled(
            Rect::from_center_size(center + Vec2::new(x, 0.), Vec2::new(4., 14.)),
            1.,
            color,
        );
    }
}

/// A dark disc with a play triangle, over a poster or a paused picture.
fn play_badge(ui: &egui::Ui, center: Pos2, diameter: f32, lit: bool) {
    let fill = if lit {
        Color32::from_rgba_unmultiplied(79, 70, 229, 235)
    } else {
        Color32::from_black_alpha(150)
    };
    ui.painter().circle_filled(center, diameter / 2., fill);
    paint_play(ui, center, diameter * 0.2, Color32::WHITE);
}

fn paint_speaker(ui: &egui::Ui, center: Pos2, color: Color32, crossed: bool) {
    let painter = ui.painter();
    let body = center + Vec2::new(-4., 0.);
    painter.rect_filled(
        Rect::from_center_size(body + Vec2::new(-3., 0.), Vec2::new(4., 6.)),
        0.,
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            body + Vec2::new(-2., -3.),
            body + Vec2::new(3., -7.),
            body + Vec2::new(3., 7.),
            body + Vec2::new(-2., 3.),
        ],
        color,
        egui::Stroke::NONE,
    ));
    let stroke = egui::Stroke::new(1.5, color);
    if crossed {
        for (from, to) in [((3., -4.), (9., 4.)), ((9., -4.), (3., 4.))] {
            painter.line_segment(
                [
                    center + Vec2::new(from.0, from.1),
                    center + Vec2::new(to.0, to.1),
                ],
                stroke,
            );
        }
    } else {
        for (x, half) in [(3., 3.), (6.5, 6.)] {
            painter.line_segment(
                [center + Vec2::new(x, -half), center + Vec2::new(x, half)],
                stroke,
            );
        }
    }
}

/// A window with an arrow leaving it.
fn paint_external(ui: &egui::Ui, center: Pos2, color: Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5, color);
    let at = |x: f32, y: f32| center + Vec2::new(x, y);
    painter.line(
        vec![
            at(-1., -6.),
            at(-7., -6.),
            at(-7., 7.),
            at(6., 7.),
            at(6., 1.),
        ],
        stroke,
    );
    painter.line_segment([at(0., 0.), at(7., -7.)], stroke);
    painter.line(vec![at(2., -7.), at(7., -7.), at(7., -2.)], stroke);
}

/// Four corners pointing out, or pointing in to leave fullscreen.
fn paint_fullscreen(ui: &egui::Ui, center: Pos2, color: Color32, leave: bool) {
    let stroke = egui::Stroke::new(1.5, color);
    for (x, y) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
        let (elbow, arm) = if leave {
            (Vec2::new(2.5 * x, 2. * y), 4.)
        } else {
            (Vec2::new(7. * x, 6. * y), -4.)
        };
        let elbow = center + elbow;
        ui.painter().line(
            vec![
                elbow + Vec2::new(arm * x, 0.),
                elbow,
                elbow + Vec2::new(0., arm * y),
            ],
            stroke,
        );
    }
}

/// A thin track filled up to `fraction`, with a handle while the pointer is on it.
fn paint_bar(ui: &egui::Ui, area: Rect, fraction: f32, lit: bool) {
    let thickness = if lit { 6. } else { 4. };
    let track = Rect::from_center_size(area.center(), Vec2::new(area.width(), thickness));
    let end = track.left() + track.width() * fraction.clamp(0., 1.);
    ui.painter().rect_filled(track, thickness / 2., TRACK);
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, Pos2::new(end, track.bottom())),
        thickness / 2.,
        PLAYED,
    );
    if lit {
        ui.painter()
            .circle_filled(Pos2::new(end, track.center().y), 7., Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_like_a_player() {
        assert_eq!(clock_text(0.), "0:00");
        assert_eq!(clock_text(65.9), "1:05");
        assert_eq!(clock_text(3665.), "1:01:05");
        assert_eq!(clock_text(-3.), "0:00");
    }

    #[test]
    fn the_picture_keeps_its_shape_inside_the_box() {
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(720., 405.));
        assert_eq!(fit(area, 16. / 9.), area);
        let portrait = fit(area, 9. / 16.);
        assert_eq!(portrait.height(), 405.);
        assert!((portrait.width() - 405. * 9. / 16.).abs() < 0.01);
        assert_eq!(portrait.center(), area.center());
        let wide = fit(area, 3.);
        assert_eq!(wide.width(), 720.);
        assert_eq!(wide.height(), 240.);
    }

    #[test]
    fn the_clock_holds_while_stopped_and_follows_corrections() {
        let mut clock = Clock {
            at: 2.,
            since: None,
        };
        assert_eq!(clock.now(), 2.);
        clock.start();
        std::thread::sleep(Duration::from_millis(20));
        assert!(clock.now() > 2.015);
        clock.set(10.);
        assert!((10. ..10.5).contains(&clock.now()), "Still running");
        clock.stop();
        let stopped = clock.now();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(clock.now(), stopped);
    }

    /// A session with no helpers, as after a video ended or was parked.
    fn parked(path: &str) -> Session {
        let (_, info_rx) = mpsc::channel();
        let now = Instant::now();
        Session {
            path: path.into(),
            decoder: None,
            audio: None,
            info: Some(Info {
                duration: Some(60.),
                audio: true,
            }),
            info_rx,
            probe_cancel: Arc::new(AtomicBool::new(false)),
            texture: None,
            aspect: None,
            next: None,
            clock: Clock {
                at: 12.,
                since: None,
            },
            playing: false,
            waiting: None,
            ended: false,
            error: None,
            shown: 12.,
            rate: playback::MAX_RATE,
            drag: None,
            last_seek: now,
            paused_at: now,
            reveal: false,
        }
    }

    fn frame(seconds: f64) -> Frame {
        Frame {
            seconds,
            width: 2,
            height: 4,
            rgba: vec![255; 32],
        }
    }

    #[test]
    fn the_position_follows_the_drag_and_stays_inside_the_video() {
        let mut session = parked("clip.mp4");
        assert_eq!(session.position(), 12.);
        session.drag = Some(30.);
        assert_eq!(session.position(), 30.);
        session.drag = Some(90.);
        assert_eq!(session.position(), 60.);
        session.info = None;
        assert_eq!(
            session.position(),
            90.,
            "Unknown length: nothing to clamp to"
        );
    }

    #[test]
    fn a_shown_frame_sets_the_shape_and_the_end_keeps_the_picture() {
        let ctx = egui::Context::default();
        let mut session = parked("clip.mp4");
        session.playing = true;
        session.clock.start();
        session.display(&ctx, frame(12.5));
        assert_eq!(session.aspect, Some(0.5));
        assert_eq!(session.shown, 12.5);
        session.finish(Err(playback::NOT_PLAYABLE.into()));
        assert!(session.ended && session.error.is_none(), "It had played");
        assert!(!session.playing);
        assert_eq!(session.position(), 60.);
        assert!(session.texture.is_some());

        let mut broken = parked("broken.mp4");
        broken.finish(Err(playback::NOT_PLAYABLE.into()));
        assert!(!broken.ended);
        assert_eq!(broken.error.as_deref(), Some(playback::NOT_PLAYABLE));
        // Nothing to play: asking again changes nothing.
        broken.set_playing(&ctx, true, 1.);
        assert!(!broken.playing && broken.decoder.is_none());
    }

    #[test]
    fn frames_wait_for_their_moment() {
        let ctx = egui::Context::default();
        let mut session = parked("clip.mp4");
        session.playing = true;
        // No decoder: each frame is one that was decoded ahead.
        session.next = Some(frame(12.));
        session.advance(&ctx, 1.);
        assert_eq!(session.shown, 12.);
        assert!(session.next.is_none());
        session.next = Some(frame(13.));
        session.advance(&ctx, 1.);
        assert_eq!(session.shown, 12., "A second ahead is not shown yet");
        assert!(session.next.is_some());
        session.clock.set(13.2);
        session.advance(&ctx, 1.);
        assert_eq!(session.shown, 13.);
    }

    #[test]
    fn opening_another_file_and_closing_replace_the_session() {
        let mut player = Player::default();
        assert_eq!(player.current(), None);
        player.session = Some(parked("first.mp4"));
        assert!(player.is_open(Path::new("first.mp4")));
        assert!(!player.is_open(Path::new("second.mp4")));
        player.suspend();
        assert!(
            player.is_open(Path::new("first.mp4")),
            "Suspending keeps it"
        );
        player.close();
        assert_eq!(player.current(), None);
    }

    #[test]
    fn the_gain_follows_the_level_and_mute() {
        let mut player = Player::default();
        assert_eq!(player.gain(), 1.);
        player.volume = 0.5;
        assert_eq!(player.gain(), 0.25);
        player.muted = true;
        assert_eq!(player.gain(), 0.);
    }

    /// Draws nothing: takes frames as the page would, until `done` or a time limit. Returns
    /// each new moment shown.
    fn follow(
        session: &mut Session,
        ctx: &egui::Context,
        done: impl Fn(&Session) -> bool,
    ) -> Vec<f64> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut moments = Vec::new();
        let mut last = None;
        while !done(session) {
            assert!(Instant::now() < deadline, "The player timed out");
            session.advance(ctx, 0.);
            if session.texture.is_some() && last != Some(session.shown) {
                last = Some(session.shown);
                moments.push(session.shown);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        moments
    }

    /// Plays real files through the real workers, at no volume. Set
    /// `STREAM_DLP_PLAYBACK_TEST_FILE` to a video of yours to also play its first seconds and
    /// print how many frames were shown.
    #[test]
    #[ignore = "requires installed FFmpeg and FFprobe"]
    fn local_player_plays_pauses_seeks_and_ends() {
        let dir = tempfile::tempdir().unwrap();
        let clip = playback::test_clip(dir.path(), "clip #1.mp4", "640x360", true);
        let ctx = egui::Context::default();
        // The first sound output of a process opens slowly; the app warms it up in time.
        let mut warm = Session::open(&ctx, clip.clone(), 0.);
        follow(&mut warm, &ctx, |session| {
            session.shown > 0.2 || session.ended
        });
        drop(warm);

        // From opening to the end, at the video's own pace.
        let started = Instant::now();
        let mut session = Session::open(&ctx, clip.clone(), 0.);
        let moments = follow(&mut session, &ctx, |session| session.ended);
        let took = started.elapsed().as_secs_f64();
        assert!(session.error.is_none());
        assert!((1.8..2.6).contains(&took), "Two seconds took {took:.2} s");
        assert!(moments.len() >= 30, "{} of 40 frames shown", moments.len());
        assert!(moments.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(session.duration(), Some(2.));
        assert!((session.aspect.unwrap() - 16. / 9.).abs() < 0.01);
        assert_eq!(session.position(), 2.);
        assert!(session.decoder.is_none() && session.audio.is_none());

        // Play after the end starts over; pause holds the picture and the position.
        session.set_playing(&ctx, true, 0.);
        follow(&mut session, &ctx, |session| {
            session.waiting.is_none() && (0.5..1.5).contains(&session.shown)
        });
        session.set_playing(&ctx, false, 0.);
        let (position, shown) = (session.position(), session.shown);
        std::thread::sleep(Duration::from_millis(200));
        session.advance(&ctx, 0.);
        assert_eq!((session.position(), session.shown), (position, shown));

        // Dragging the bar shows the moment under it without playing on.
        std::thread::sleep(SCRUB_EVERY);
        session.scrub(&ctx, 1.5, false, 0.);
        follow(&mut session, &ctx, |session| session.waiting.is_none());
        assert!((session.shown - 1.5).abs() <= 0.05, "{}", session.shown);
        assert_eq!(session.position(), 1.5);
        assert!(session.audio.is_none(), "No sound while dragging");
        session.scrub(&ctx, 1., true, 0.);
        follow(&mut session, &ctx, |session| session.waiting.is_none());
        assert!((session.shown - 1.).abs() <= 0.05, "{}", session.shown);
        assert!(!session.playing, "A paused video stays paused after a seek");

        // A parked video starts its helpers again where it was.
        session.park();
        assert!(session.decoder.is_none());
        let resumed = Instant::now();
        session.set_playing(&ctx, true, 0.);
        let moments = follow(&mut session, &ctx, |session| session.ended);
        let took = resumed.elapsed().as_secs_f64();
        assert!(moments[0] >= 0.95, "Resumed at {}", moments[0]);
        assert!(
            (0.8..3.).contains(&took),
            "The last second took {took:.2} s"
        );

        // A file that is not a video says so.
        let broken = dir.path().join("broken.mp4");
        std::fs::write(&broken, b"not video").unwrap();
        let mut session = Session::open(&ctx, broken, 0.);
        follow(&mut session, &ctx, |session| session.error.is_some());
        assert!(!session.playing && !session.ended);

        if let Some(file) = std::env::var_os("STREAM_DLP_PLAYBACK_TEST_FILE") {
            let started = Instant::now();
            let mut session = Session::open(&ctx, file.into(), 0.);
            let moments = follow(&mut session, &ctx, |session| {
                session.ended || session.error.is_some() || session.shown >= 5.
            });
            assert!(session.error.is_none(), "{:?}", session.error);
            println!(
                "{} frames shown up to {:.2} s of video in {:.2} s, last asked for {} a second; {:?}, sound: {}",
                moments.len(),
                session.shown,
                started.elapsed().as_secs_f64(),
                session.rate,
                session.info,
                session
                    .audio
                    .as_ref()
                    .is_some_and(|a| a.position().is_some()),
            );
        }
    }

    /// The fullscreen changes a pass asked the window for.
    fn fullscreen_commands(output: &egui::FullOutput) -> Vec<bool> {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| {
                viewport
                    .commands
                    .iter()
                    .filter_map(|command| match command {
                        egui::ViewportCommand::Fullscreen(on) => Some(*on),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn with_video(path: &str) -> Player {
        Player {
            session: Some(parked(path)),
            ..Default::default()
        }
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// One pass that draws `player` for `path` when given one, with `events` as its input.
    /// Returns the fullscreen changes asked for and whether Escape was left for the rest of
    /// the app.
    fn pass(
        ctx: &egui::Context,
        player: &mut Player,
        path: Option<&str>,
        events: Vec<egui::Event>,
    ) -> (Vec<bool>, bool) {
        let mut previews = Previews::default();
        let mut escape_left = false;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(path) = path {
                    player.show(ui, &mut previews, Path::new(path));
                }
            });
            escape_left = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        });
        (fullscreen_commands(&output), escape_left)
    }

    #[test]
    fn fullscreen_turns_on_and_off_and_needs_a_video() {
        let ctx = egui::Context::default();
        let mut player = Player::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            player.set_fullscreen(ctx, true);
        });
        assert!(player.fullscreen.is_none(), "Nothing open");
        assert!(fullscreen_commands(&output).is_empty());

        player.session = Some(parked("clip.mp4"));
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            player.set_fullscreen(ctx, true);
            player.set_fullscreen(ctx, true);
        });
        assert!(player.fullscreen.is_some());
        assert_eq!(fullscreen_commands(&output), [true], "Asked once");
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            player.set_fullscreen(ctx, false);
            player.set_fullscreen(ctx, false);
        });
        assert!(player.fullscreen.is_none());
        assert_eq!(fullscreen_commands(&output), [false]);
        assert!(
            player.is_open(Path::new("clip.mp4")),
            "The video stays open"
        );
    }

    #[test]
    fn keys_toggle_fullscreen_and_escape_only_leaves_it() {
        let ctx = egui::Context::default();
        let mut player = Player::default();
        let mut session = parked("clip.mp4");
        session.playing = true;
        player.session = Some(session);
        let path = Some("clip.mp4");
        // On the page, the keys need the pointer over the player; fullscreen they do not.
        player.set_fullscreen(&ctx, true);
        let _ = pass(&ctx, &mut player, path, Vec::new());

        let (commands, escape_left) = pass(&ctx, &mut player, path, vec![key(egui::Key::Escape)]);
        assert_eq!(commands, [false]);
        assert!(!escape_left, "Escape did nothing else");
        assert!(player.fullscreen.is_none());
        assert!(
            player.session.as_ref().unwrap().playing,
            "Leaving fullscreen does not pause"
        );

        let (_, escape_left) = pass(&ctx, &mut player, path, vec![key(egui::Key::Escape)]);
        assert!(
            escape_left,
            "Not fullscreen: Escape is the rest of the app's"
        );

        player.set_fullscreen(&ctx, true);
        let _ = pass(&ctx, &mut player, path, Vec::new());
        let (commands, _) = pass(&ctx, &mut player, path, vec![key(egui::Key::F)]);
        assert_eq!(commands, [false], "F leaves fullscreen too");
        assert!(player.fullscreen.is_none());
    }

    #[test]
    fn a_fullscreen_player_that_is_not_drawn_restores_the_window() {
        let ctx = egui::Context::default();
        let mut player = with_video("clip.mp4");
        player.set_fullscreen(&ctx, true);
        let (commands, _) = pass(&ctx, &mut player, Some("clip.mp4"), Vec::new());
        assert_eq!(commands, [true], "Drawn: stays fullscreen");
        let (commands, _) = pass(&ctx, &mut player, None, Vec::new());
        assert_eq!(commands, [false], "Not drawn: the window is restored");
        let (commands, _) = pass(&ctx, &mut player, Some("clip.mp4"), Vec::new());
        assert!(commands.is_empty());
        assert!(player.fullscreen.is_none(), "Back on the page");
    }

    #[test]
    fn closing_suspending_or_another_video_leave_fullscreen() {
        let ctx = egui::Context::default();
        for leave in [Player::close, Player::suspend] {
            let mut player = with_video("clip.mp4");
            player.set_fullscreen(&ctx, true);
            leave(&mut player);
            assert!(player.fullscreen.is_none());
            assert!(
                ctx.data(|data| data.get_temp::<Drawn>(drawn_id()))
                    .is_none()
            );
        }
        let mut player = with_video("clip.mp4");
        player.set_fullscreen(&ctx, true);
        let _ = pass(&ctx, &mut player, Some("clip.mp4"), Vec::new());
        let (commands, _) = pass(&ctx, &mut player, Some("other.mp4"), Vec::new());
        assert_eq!(commands, [false]);
        assert!(player.fullscreen.is_none() && player.current().is_none());
    }

    #[test]
    fn fullscreen_controls_hide_after_a_while_unless_paused_or_held() {
        let start = Instant::now();
        let state = Fullscreen {
            ctx: egui::Context::default(),
            active: start,
        };
        let later = |millis| start + Duration::from_millis(millis);
        assert!(state.controls_shown(later(2400), true, false));
        assert!(!state.controls_shown(later(2500), true, false));
        assert!(state.controls_shown(later(9000), false, false), "Paused");
        assert!(state.controls_shown(later(9000), true, true), "Held");
        assert!(state.controls_shown(start, true, false));
    }

    #[test]
    fn movement_clicks_and_keys_wake_the_controls() {
        let ctx = egui::Context::default();
        let stirred_by = |events: Vec<egui::Event>| {
            let mut stirred_now = false;
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| stirred_now = ctx.input(stirred),
            );
            stirred_now
        };
        let _ = stirred_by(vec![egui::Event::PointerMoved(Pos2::new(10., 10.))]);
        assert!(!stirred_by(Vec::new()), "Nothing happened");
        assert!(stirred_by(vec![egui::Event::PointerMoved(Pos2::new(
            40., 10.
        ))]));
        assert!(
            !stirred_by(vec![egui::Event::PointerMoved(Pos2::new(40., 10.))]),
            "Same place"
        );
        assert!(stirred_by(vec![key(egui::Key::A)]));
    }

    #[test]
    fn the_system_player_pauses_this_one_and_leaves_fullscreen() {
        let ctx = egui::Context::default();
        let mut player = Player::default();
        let mut session = parked("clip.mp4");
        session.playing = true;
        player.session = Some(session);
        player.set_fullscreen(&ctx, true);
        let mut opened = None;
        player.open_externally(&ctx, |path| {
            opened = Some(path.to_owned());
            Ok(())
        });
        assert_eq!(opened.as_deref(), Some(Path::new("clip.mp4")));
        assert!(!player.session.as_ref().unwrap().playing);
        assert!(player.fullscreen.is_none());
        assert_eq!(player.error, None);

        player.open_externally(&ctx, |_| Err("Moved".into()));
        assert_eq!(player.error.as_deref(), Some("Moved"));
    }

    #[test]
    fn the_poster_and_the_player_draw() {
        let ctx = egui::Context::default();
        let mut previews = Previews::default();
        let mut player = Player::default();
        let mut draw = |player: &mut Player, path: &str| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    player.show(ui, &mut previews, Path::new(path));
                });
            });
        };
        draw(&mut player, "missing.mp4");
        assert_eq!(player.current(), None, "Only a click opens it");
        for (ended, error) in [(false, None), (true, None), (false, Some("Failed"))] {
            let mut session = parked("missing.mp4");
            session.ended = ended;
            session.error = error.map(str::to_owned);
            session.display(&ctx, frame(1.));
            player.session = Some(session);
            draw(&mut player, "missing.mp4");
            assert!(player.is_open(Path::new("missing.mp4")));
        }
        draw(&mut player, "other.mp4");
        assert_eq!(player.current(), None, "Another file closes the open one");
    }
}
