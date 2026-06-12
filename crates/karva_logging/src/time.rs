use std::time::Duration;

pub fn format_duration(duration: Duration) -> String {
    if duration.as_secs() < 2 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

/// Format a duration in nextest-style bracketed format: `[   0.015s]`.
pub fn format_duration_bracketed(duration: Duration) -> String {
    format!("[{:>8.3}s]", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn format_duration_zero_is_zero_ms() {
        assert_snapshot!(format_duration(Duration::ZERO), @"0ms");
    }

    #[test]
    fn format_duration_sub_millisecond_truncates_to_zero_ms() {
        assert_snapshot!(format_duration(Duration::from_micros(500)), @"0ms");
        assert_snapshot!(format_duration(Duration::from_nanos(1)), @"0ms");
    }

    #[test]
    fn format_duration_exactly_one_ms() {
        assert_snapshot!(format_duration(Duration::from_millis(1)), @"1ms");
    }

    /// The cutoff is `< 2s`, so anything under two full seconds stays in ms,
    /// including the exact one-second boundary and values like 1999 ms.
    #[test]
    fn format_duration_sub_two_seconds_uses_milliseconds() {
        assert_snapshot!(format_duration(Duration::from_secs(1)), @"1000ms");
        assert_snapshot!(format_duration(Duration::from_millis(1999)), @"1999ms");
    }

    #[test]
    fn format_duration_two_seconds_switches_to_seconds() {
        assert_snapshot!(format_duration(Duration::from_secs(2)), @"2.00s");
    }

    #[test]
    fn format_duration_rounds_to_two_decimals() {
        assert_snapshot!(format_duration(Duration::from_millis(2346)), @"2.35s");
        assert_snapshot!(format_duration(Duration::from_millis(2344)), @"2.34s");
    }

    #[test]
    fn format_duration_minutes_stay_in_seconds() {
        assert_snapshot!(format_duration(Duration::from_secs(125)), @"125.00s");
    }

    #[test]
    fn format_duration_bracketed_pads_to_width_and_three_decimals() {
        assert_snapshot!(format_duration_bracketed(Duration::ZERO), @"[   0.000s]");
        assert_snapshot!(format_duration_bracketed(Duration::from_millis(15)), @"[   0.015s]");
    }

    #[test]
    fn format_duration_bracketed_handles_large_values() {
        assert_snapshot!(format_duration_bracketed(Duration::from_secs(12345)), @"[12345.000s]");
    }
}
