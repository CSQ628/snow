//! A clip is a half-open interval on the original, pause-free media clock.

/// The editor's exact, finite set of playback rates. Timing never uses floating point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackSpeed(u32);

impl Default for PlaybackSpeed {
    fn default() -> Self {
        Self(100)
    }
}

impl PlaybackSpeed {
    pub fn from_percent(percent: u32) -> Result<Self, String> {
        if matches!(percent, 25 | 50 | 75 | 100 | 125 | 150 | 175 | 200 | 300) {
            Ok(Self(percent))
        } else {
            Err("unsupported playback speed".into())
        }
    }

    pub fn percent(self) -> u32 {
        self.0
    }

    pub fn source_elapsed_us(self, elapsed_us: u64) -> u64 {
        (u128::from(elapsed_us) * u128::from(self.0) / 100).min(u128::from(u64::MAX)) as u64
    }

    pub fn output_duration_us(self, source_us: u64) -> Result<u64, String> {
        u64::try_from((u128::from(source_us) * 100).div_ceil(u128::from(self.0)))
            .map_err(|_| "retimed duration overflow".into())
    }

    /// Count presentation slots before rounding the retimed duration to microseconds.
    pub fn output_frame_count(self, source_us: u64, fps: u32) -> Result<u64, String> {
        if fps == 0 {
            return Err("retimed frame rate must be positive".into());
        }
        u64::try_from(
            (u128::from(source_us) * u128::from(fps) * 100)
                .div_ceil(1_000_000 * u128::from(self.0)),
        )
        .map_err(|_| "retimed frame count overflow".into())
    }

    /// Direct rational mapping avoids first rounding an output slot to microseconds.
    pub fn source_time_at_frame(self, index: u64, fps: u32) -> Result<u64, String> {
        if fps == 0 {
            return Err("retimed frame rate must be positive".into());
        }
        u64::try_from(u128::from(index) * 1_000_000 * u128::from(self.0) / (u128::from(fps) * 100))
            .map_err(|_| "retimed source timestamp overflow".into())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrimRange {
    pub start_us: u64,
    pub end_us: u64,
}

impl TrimRange {
    pub fn new(start_us: u64, end_us: u64, duration_us: u64) -> Result<Self, String> {
        if start_us >= end_us || end_us > duration_us {
            return Err("trim range must contain media and stay inside the source".into());
        }
        Ok(Self { start_us, end_us })
    }

    pub fn duration_us(self) -> u64 {
        self.end_us - self.start_us
    }

    pub fn sample_count(self, rate: u32) -> u64 {
        (u128::from(self.end_us) * u128::from(rate) / 1_000_000) as u64 - self.sample_offset(rate)
    }

    pub fn sample_offset(self, rate: u32) -> u64 {
        (u128::from(self.start_us) * u128::from(rate) / 1_000_000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_speed_accepts_only_the_editor_rates_and_maps_rational_time() {
        assert_eq!(PlaybackSpeed::default().percent(), 100);
        for percent in [25, 50, 75, 100, 125, 150, 175, 200, 300] {
            let speed = PlaybackSpeed::from_percent(percent).unwrap();
            assert_eq!(
                speed.source_elapsed_us(1_000_000),
                u64::from(percent) * 10_000
            );
            assert_eq!(
                speed.output_duration_us(1_000_001).unwrap(),
                (100_000_100u64).div_ceil(u64::from(percent))
            );
            assert_eq!(
                speed.source_time_at_frame(83, 83).unwrap(),
                u64::from(percent) * 10_000
            );
        }
        for percent in [0, 24, 26, 99, 101, 250, 301, u32::MAX] {
            assert!(PlaybackSpeed::from_percent(percent).is_err());
        }
        let slow = PlaybackSpeed::from_percent(25).unwrap();
        assert!(slow.output_duration_us(u64::MAX).is_err());
        assert_eq!(slow.output_duration_us(1).unwrap(), 4);
        assert_eq!(
            PlaybackSpeed::from_percent(300)
                .unwrap()
                .output_duration_us(1)
                .unwrap(),
            1
        );
        assert!(slow.source_time_at_frame(1, 0).is_err());
        assert!(
            PlaybackSpeed::from_percent(300)
                .unwrap()
                .source_time_at_frame(u64::MAX, 1)
                .is_err()
        );
    }

    #[test]
    fn half_open_range_rejects_empty_reversed_and_outside_media() {
        for (start, end) in [(0, 0), (10, 9), (0, 101)] {
            assert!(TrimRange::new(start, end, 100).is_err());
        }
        assert_eq!(TrimRange::new(0, 100, 100).unwrap().duration_us(), 100);
        assert_eq!(TrimRange::new(99, 100, 100).unwrap().duration_us(), 1);
    }

    #[test]
    fn output_frame_count_does_not_round_through_the_microsecond_clock() {
        let fast = PlaybackSpeed::from_percent(300).unwrap();
        assert_eq!(fast.output_duration_us(1_000_000).unwrap(), 333_334);
        assert_eq!(fast.output_frame_count(1_000_000, 60).unwrap(), 20);
        assert_eq!(
            PlaybackSpeed::from_percent(125)
                .unwrap()
                .output_frame_count(83_333, 60)
                .unwrap(),
            4
        );
        for percent in [25, 50, 75, 100, 125, 150, 175, 200, 300] {
            let speed = PlaybackSpeed::from_percent(percent).unwrap();
            assert_eq!(speed.output_frame_count(1, 60).unwrap(), 1);
            assert_eq!(speed.output_frame_count(0, 60).unwrap(), 0);
            assert_eq!(
                speed.output_frame_count(1_000_000, 60).unwrap(),
                6000u64.div_ceil(u64::from(percent))
            );
            assert!(speed.output_frame_count(1, 0).is_err());
        }
        assert!(
            PlaybackSpeed::from_percent(25)
                .unwrap()
                .output_frame_count(u64::MAX, u32::MAX)
                .is_err()
        );
    }

    #[test]
    fn audio_offsets_use_rational_media_time_without_overflow() {
        let range = TrimRange::new(1_234_567, 2_000_000, 2_000_000).unwrap();
        assert_eq!(range.sample_offset(48_000), 59_259);
        let range = TrimRange::new(u64::MAX / 2, u64::MAX, u64::MAX).unwrap();
        assert_eq!(range.sample_offset(1_000_000), range.start_us);
    }
}
