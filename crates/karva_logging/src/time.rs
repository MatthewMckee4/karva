use std::time::Duration;

pub fn format_duration(duration: Duration) -> String {
    if duration.as_secs() < 2 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}
