//! The download engine: links, video details and formats, and running yt-dlp. No egui.
mod error;
mod media;
mod sources;
mod ytdlp;

pub use error::EngineError;
#[cfg(test)]
pub use media::parse_video;
pub use media::{CompletedDownload, Format, MediaIdentity, Video};
pub use sources::{Link, Source, parse_link};
pub use ytdlp::{Event, download, inspect, progress};
