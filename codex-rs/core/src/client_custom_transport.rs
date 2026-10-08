//! Transport policy for the custom distribution, isolated from upstream routing and retries.

// Keep the upstream HTTP implementation intact, but never activate it for inference.
// The fallback gate also leaves WebSockets enabled for later requests in the same session.
pub(super) const ALLOW_HTTP_INFERENCE: bool = false;
pub(super) const HTTP_DISABLED_MESSAGE: &str = "HTTP inference is disabled in this custom build. Check the WebSocket connection and send your message again.";
