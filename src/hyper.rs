use std::time::Duration;

use rust_extensions::date_time::DateTimeAsMicroseconds;

#[derive(Debug)]
pub enum SendHyperPayloadError {
    Disconnected,
    Disposed,
    RequestTimeout(Duration),
    HyperError {
        connected: DateTimeAsMicroseconds,
        err: hyper::Error,
    },
}

pub const HYPER_INIT_TIMEOUT: Duration = Duration::from_secs(5);
