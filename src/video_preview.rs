//! Lazy, bounded frame previews. No decoding or file reads on the UI thread.
use crate::process;
use eframe::egui;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const WIDTH: usize = 480;
const HEIGHT: usize = 270;
const FRAME_BYTES: usize = WIDTH * HEIGHT * 3;
const CACHE_LIMIT: usize = 24;

#[derive(Clone, PartialEq, Eq)]
struct Key(PathBuf, Option<u64>);
impl Key {
    fn new(path: &Path, seconds: Option<f64>) -> Self {
        Self(
            path.to_owned(),
            seconds.map(|s| (s.max(0.) * 10.).round() as u64),
        )
    }
}
type FrameResult = (u64, Key, Result<Vec<u8>, String>);

pub struct Previews {
    cache: VecDeque<(Key, Result<egui::TextureHandle, String>)>,
    tx: mpsc::Sender<FrameResult>,
    rx: mpsc::Receiver<FrameResult>,
    active: bool,
    generation: u64,
    cancel: Arc<AtomicBool>,
}
impl Default for Previews {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cache: VecDeque::new(),
            tx,
            rx,
            active: false,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Drop for Previews {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Previews {
    pub fn clear(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.generation += 1;
        self.cache.clear();
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        seconds: Option<f64>,
        size: egui::Vec2,
    ) -> egui::Response {
        while let Ok((generation, key, result)) = self.rx.try_recv() {
            self.active = false;
            if generation == self.generation {
                let texture = result.map(|rgb| {
                    ui.ctx().load_texture(
                        "video-preview",
                        egui::ColorImage::from_rgb([WIDTH, HEIGHT], &rgb),
                        egui::TextureOptions::LINEAR,
                    )
                });
                self.cache.push_back((key, texture));
                while self.cache.len() > CACHE_LIMIT {
                    self.cache.pop_front();
                }
            }
        }
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let key = Key::new(path, seconds);
        ui.painter()
            .rect_filled(rect, 6., egui::Color32::from_rgb(25, 28, 35));
        if let Some(index) = self.cache.iter().position(|(k, _)| *k == key) {
            let entry = self.cache.remove(index).unwrap();
            match &entry.1 {
                Ok(texture) => {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                        egui::Color32::WHITE,
                    );
                }
                Err(error) => {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "No preview",
                        egui::FontId::proportional(12.),
                        egui::Color32::LIGHT_GRAY,
                    );
                    self.cache.push_back(entry.clone());
                    return response.on_hover_text(error);
                }
            }
            self.cache.push_back(entry);
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Loading…",
                egui::FontId::proportional(12.),
                egui::Color32::LIGHT_GRAY,
            );
            if !self.active {
                self.active = true;
                self.cancel = Arc::new(AtomicBool::new(false));
                let (tx, cancel, generation, ctx) = (
                    self.tx.clone(),
                    self.cancel.clone(),
                    self.generation,
                    ui.ctx().clone(),
                );
                std::thread::spawn(move || {
                    let result = extract(&key.0, key.1.map(|s| s as f64 / 10.), &cancel);
                    let _ = tx.send((generation, key, result));
                    ctx.request_repaint();
                });
            }
            // Also covers a worker completing between polling and painting.
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        response
    }
}

fn extract(path: &Path, seconds: Option<f64>, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    // Scale before sampling a representative poster, keeping the sample buffer small.
    let mut filter = "scale=w='max(1,min(480,270*dar))':h='max(1,min(270,480/dar))',setsar=1,pad=480:270:(ow-iw)/2:(oh-ih)/2".to_string();
    if seconds.is_none() {
        filter.push_str(",thumbnail=30");
    }
    let seek = format!("{:.1}", seconds.unwrap_or(0.));
    let output = process::Helper::new(
        Command::new(crate::tools::path("ffmpeg"))
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-protocol_whitelist",
                "file",
                "-ss",
                &seek,
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:V:0",
                "-frames:v",
                "1",
                "-an",
                "-sn",
                "-vf",
                &filter,
                "-threads",
                "1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ]),
        cancel,
    )
    .time_limit(Duration::from_secs(10))
    .output_limit(FRAME_BYTES as u64)
    .ignore_stderr()
    .output()
    .map_err(|error| match error {
        process::Error::Start(_) => "Install the download tools to enable video previews.".into(),
        process::Error::Cancelled | process::Error::TimedOut | process::Error::TooMuchOutput => {
            "Preview cancelled or timed out. You can still open the video.".into()
        }
        process::Error::Wait(error) => error.to_string(),
    })?;
    if !output.status.success() || output.stdout.len() != FRAME_BYTES {
        return Err("This frame could not be previewed. You can still open the video.".into());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_results_are_discarded_and_texture_cache_is_bounded() {
        let mut previews = Previews::default();
        previews.active = true;
        previews.clear();
        // An invalid old frame must be discarded before attempting texture upload.
        previews
            .tx
            .send((0, Key::new(Path::new("old"), None), Ok(vec![])))
            .unwrap();
        for i in 0..30 {
            previews
                .tx
                .send((
                    1,
                    Key::new(Path::new(&format!("{i}")), None),
                    Ok(vec![0; FRAME_BYTES]),
                ))
                .unwrap();
        }
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                previews.show(ui, Path::new("29"), None, egui::vec2(136., 76.5));
            });
        });
        assert_eq!(previews.cache.len(), CACHE_LIMIT);
        assert!(!previews.active);
        assert!(
            previews
                .cache
                .iter()
                .all(|(key, _)| key.0 != Path::new("old"))
        );
        previews.clear();
        assert!(previews.cache.is_empty());
    }

    #[test]
    fn ffmpeg_previews_seek_letterbox_and_handle_short_or_invalid_sources() {
        let exe = crate::tools::path("ffmpeg");
        if !exe.exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portrait clip #1 🐦.mp4");
        let status = process::isolate(
            Command::new(exe)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=90x160:rate=10",
                    "-t",
                    "0.8",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&path),
        )
        .status()
        .unwrap();
        assert!(status.success());
        let cancel = AtomicBool::new(false);
        let first = extract(&path, Some(0.), &cancel).unwrap();
        let later = extract(&path, Some(0.5), &cancel).unwrap();
        assert_eq!(first.len(), FRAME_BYTES);
        assert_ne!(first, later, "Seeking must display a different moment");
        assert!(
            first[..3].iter().all(|v| *v < 5),
            "Portrait video has side bars"
        );
        assert!(
            first.chunks_exact(3).any(|p| p[0] > 100),
            "Preview contains video pixels"
        );
        assert_eq!(
            extract(&path, None, &cancel).unwrap().len(),
            FRAME_BYTES,
            "Poster sampling must flush clips with fewer than 30 frames"
        );
        assert!(extract(&path, Some(99.), &cancel).is_err());
        let invalid = dir.path().join("broken.mp4");
        std::fs::write(&invalid, b"not video").unwrap();
        assert!(extract(&invalid, None, &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(extract(&path, None, &cancel).is_err());
    }
}
