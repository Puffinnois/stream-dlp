//! What can go wrong while finding or downloading a video, and what to tell the user.
use super::Source;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The link, or what it leads to, is not something stream-dlp downloads.
    Unsupported(String),
    /// The site asks for a signed-in session or a verification check.
    SignInNeeded(Source),
    RateLimited(Source),
    /// The video is private, removed, or otherwise gone.
    Unavailable(Source),
    /// The site refused this request.
    Restricted(Source),
    /// The site could not be reached.
    Network(Source),
    /// The site took too long to answer.
    TimedOut,
    /// A helper failed; the text is safe to show.
    Helper(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Helper(message) => f.write_str(message),
            Self::SignInNeeded(Source::YouTube) => f.write_str("YouTube requires sign-in or a verification check. Use Connect browser / retry help to import your YouTube session, then retry. YouTube may still restrict this connection or video."),
            Self::SignInNeeded(Source::TikTok) => f.write_str("TikTok requires access or verification for this video. Only public videos are supported; YouTube browser sessions cannot unlock TikTok videos."),
            Self::RateLimited(source) => write!(
                f,
                "{} is limiting requests. Wait a while before trying again.",
                source.label()
            ),
            Self::Unavailable(source) => write!(
                f,
                "This {} video is unavailable or has been removed. Check that it still plays in your browser.",
                source.label()
            ),
            Self::Restricted(source) => write!(
                f,
                "{} restricted this request. Try again later or check the video in your browser.",
                source.label()
            ),
            Self::Network(source) => write!(
                f,
                "Could not connect to {}. Check your connection and try again.",
                source.label()
            ),
            Self::TimedOut => f.write_str("The video service took too long to respond. Check your connection and try again."),
        }
    }
}

impl EngineError {
    pub fn user_message(&self) -> String {
        self.to_string()
    }

    /// Whether importing the YouTube browser session could help; the UI keys its Firefox
    /// auto-refresh on it.
    pub fn needs_session(&self) -> bool {
        *self == Self::SignInNeeded(Source::YouTube)
    }
}

impl From<String> for EngineError {
    fn from(message: String) -> Self {
        Self::Helper(message)
    }
}
