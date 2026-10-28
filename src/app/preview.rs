//! Lazy, bounded frame previews. No decoding or file reads on the UI thread.
use crate::media::preview::{HEIGHT, WIDTH, extract};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::preview::FRAME_BYTES;

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
}
