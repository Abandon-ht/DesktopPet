//! Render cadence is a resource budget, not a promise of completed display FPS.
use anyhow::{Context, Result, ensure};
use std::time::Duration;

pub fn frame_interval() -> Result<Duration> {
    let value = std::env::var("DESKTOPPET_MAX_FPS").ok();
    parse_frame_interval(value.as_deref())
}

fn parse_frame_interval(value: Option<&str>) -> Result<Duration> {
    let fps: u32 = value
        .unwrap_or("30")
        .parse()
        .context("DESKTOPPET_MAX_FPS must be an integer from 1 to 60")?;
    ensure!(
        (1..=60).contains(&fps),
        "DESKTOPPET_MAX_FPS must be from 1 to 60"
    );
    Ok(Duration::from_secs_f64(1.0 / f64::from(fps)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cadence_stays_unchanged_and_invalid_budgets_are_rejected() {
        assert_eq!(
            parse_frame_interval(None).unwrap(),
            Duration::from_secs_f64(1.0 / 30.0)
        );
        assert_eq!(
            parse_frame_interval(Some("20")).unwrap(),
            Duration::from_millis(50)
        );
        for value in ["0", "61", "-1", "NaN", "20.5"] {
            assert!(parse_frame_interval(Some(value)).is_err());
        }
    }
}
