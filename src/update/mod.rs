//! App updates: the release channel, and finding and installing new versions. Windows
//! installs updates; the experimental Mac release points to Releases.
mod channel;
#[cfg(not(windows))]
mod manual;
#[cfg(windows)]
mod windows;

pub use channel::Channel;
#[cfg(not(windows))]
pub use manual::{RELEASES, Updater, remove_stale_installers};
#[cfg(windows)]
pub use windows::{RELEASES, Updater, remove_stale_installers};
