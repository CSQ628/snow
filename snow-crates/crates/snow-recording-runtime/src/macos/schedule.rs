//! Capture observations and output deadlines have independent ownership.
use std::time::Duration;

use crate::output_schedule::{OutputSchedule, OutputSlot};

pub(super) struct ScheduledFrame<T> {
    pub frame: T,
    pub revision: u64,
    pub slot: OutputSlot,
}

pub(super) struct VideoSchedule<T> {
    output: OutputSchedule,
    pending: Option<(Duration, u64, T)>,
    current: Option<(u64, T)>,
    revision: u64,
}

impl<T: Clone> VideoSchedule<T> {
    pub fn new(fps: u32) -> Self {
        Self {
            output: OutputSchedule::new(fps),
            pending: None,
            current: None,
            revision: 0,
        }
    }

    pub fn wait(&self, elapsed: Duration, limit: Duration) -> Duration {
        self.output.wait(elapsed).min(limit)
    }

    /// Admit an observation and select the latest due output slot together.
    /// Observations must not be later than `elapsed`. A pending image due at
    /// this slot is selected before a newer, future image can supersede it.
    /// Between slots only the newest observation can affect a later output, so
    /// the producer keeps capacity without retaining a queue of native leases.
    pub fn poll(
        &mut self,
        elapsed: Duration,
        observation: Option<(Duration, T)>,
    ) -> Option<ScheduledFrame<T>> {
        let slot = self.output.poll(elapsed);
        if self
            .pending
            .as_ref()
            .is_some_and(|(at, _, _)| slot.is_some_and(|slot| *at <= slot.at))
        {
            let (_, revision, frame) = self.pending.take().expect("pending observation");
            self.current = Some((revision, frame));
        }
        if let Some((at, frame)) = observation {
            debug_assert!(at <= elapsed);
            self.revision = self
                .revision
                .checked_add(1)
                .expect("capture revision overflow");
            if slot.is_some_and(|slot| at <= slot.at) {
                self.current = Some((self.revision, frame));
            } else {
                self.pending = Some((at, self.revision, frame));
            }
        }
        let slot = slot?;
        let (revision, frame) = self.current.as_ref()?;
        Some(ScheduledFrame {
            frame: frame.clone(),
            revision: *revision,
            slot,
        })
    }

    /// Pauses, interruptions and configuration changes invalidate image leases,
    /// while the active-time output schedule and generation counter survive.
    pub fn clear(&mut self) {
        self.pending = None;
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_capture_drives_every_output_deadline_without_twenty_millisecond_polling() {
        for fps in [30, 60, 120, 144, 240] {
            let mut schedule = VideoSchedule::new(fps);
            let mut now = Duration::ZERO;
            let mut output = Vec::new();
            let mut observation = Some((Duration::ZERO, "desktop"));
            while now < Duration::from_secs(1) {
                if let Some(frame) = schedule.poll(now, observation.take()) {
                    assert_eq!(frame.frame, "desktop");
                    assert_eq!(frame.revision, 1);
                    output.push(frame.slot.pts);
                }
                let wait = schedule.wait(now, Duration::from_millis(20));
                assert!(!wait.is_zero());
                now += wait;
            }
            assert_eq!(output, (0..u64::from(fps)).collect::<Vec<_>>());
        }
    }

    #[test]
    fn new_captures_wait_for_their_slot_and_held_frames_keep_their_revision() {
        let mut schedule = VideoSchedule::new(60);
        assert_eq!(
            schedule
                .poll(Duration::ZERO, Some((Duration::ZERO, 1)))
                .unwrap()
                .frame,
            1
        );
        assert_eq!(
            schedule
                .poll(
                    Duration::from_millis(21),
                    Some((Duration::from_millis(20), 2)),
                )
                .unwrap()
                .frame,
            1
        );
        let changed = schedule.poll(Duration::from_millis(34), None).unwrap();
        assert_eq!(
            (changed.frame, changed.revision, changed.slot.pts),
            (2, 2, 2)
        );
        assert_eq!(
            schedule
                .poll(Duration::from_millis(51), None)
                .unwrap()
                .revision,
            2
        );
        assert!(schedule.poll(Duration::from_millis(51), None).is_none());
    }

    #[test]
    fn overload_selects_latest_due_image_and_never_replays_missed_slots() {
        let mut schedule = VideoSchedule::new(60);
        schedule.poll(Duration::ZERO, Some((Duration::ZERO, 0)));
        schedule.poll(
            Duration::from_millis(5),
            Some((Duration::from_millis(5), 1)),
        );
        let selected = schedule
            .poll(
                Duration::from_millis(100),
                Some((Duration::from_millis(95), 19)),
            )
            .unwrap();
        assert_eq!((selected.frame, selected.slot.pts), (19, 6));
        assert!(schedule.poll(Duration::from_millis(100), None).is_none());
    }

    #[test]
    fn interruption_discards_old_and_pending_images_without_resetting_pts() {
        let mut schedule = VideoSchedule::new(60);
        schedule
            .poll(Duration::ZERO, Some((Duration::ZERO, 1)))
            .unwrap();
        schedule.poll(
            Duration::from_millis(10),
            Some((Duration::from_millis(10), 2)),
        );
        schedule.clear();
        assert!(schedule.poll(Duration::from_millis(17), None).is_none());
        let resumed = schedule
            .poll(
                Duration::from_millis(34),
                Some((Duration::from_millis(25), 3)),
            )
            .unwrap();
        assert_eq!(
            (resumed.frame, resumed.revision, resumed.slot.pts),
            (3, 3, 2)
        );
    }

    #[test]
    fn due_capture_survives_a_newer_capture_after_the_output_boundary() {
        let mut schedule = VideoSchedule::new(60);
        schedule.poll(Duration::ZERO, Some((Duration::ZERO, 0)));
        assert!(
            schedule
                .poll(
                    Duration::from_millis(10),
                    Some((Duration::from_millis(10), 1)),
                )
                .is_none()
        );
        let first = schedule
            .poll(
                Duration::from_millis(21),
                Some((Duration::from_millis(20), 2)),
            )
            .unwrap();
        assert_eq!((first.frame, first.slot.pts), (1, 1));
        let second = schedule
            .poll(
                Duration::from_millis(34),
                Some((Duration::from_millis(34), 3)),
            )
            .unwrap();
        assert_eq!((second.frame, second.slot.pts), (2, 2));
        let third = schedule.poll(Duration::from_millis(51), None).unwrap();
        assert_eq!((third.frame, third.slot.pts), (3, 3));
    }

    #[test]
    fn faster_capture_recycles_native_pool_and_preserves_the_final_update() {
        let size = snow_media::geometry::PixelSize::new(16, 16).unwrap();
        let mut producer =
            snow_macos::compositor::Compositor::new(size, snow_media::PixelFormat::Bgra8, 4)
                .unwrap();
        let mut schedule = VideoSchedule::new(1);
        let mut latest_capture = None;
        let mut output = Vec::new();
        for milliseconds in 0..=3000 {
            let now = Duration::from_millis(milliseconds);
            let observation = if milliseconds > 0 && milliseconds < 2000 && milliseconds % 17 == 0 {
                let image = producer.compose(&[], true).unwrap();
                latest_capture = Some(milliseconds);
                Some((now, (milliseconds, image)))
            } else {
                None
            };
            if let Some(frame) = schedule.poll(now, observation) {
                assert_eq!(Some(frame.frame.0), latest_capture);
                output.push((frame.slot.pts, frame.frame.0, frame.revision));
            }
        }
        assert_eq!(output.len(), 3);
        assert_eq!(output[0].1, 986);
        assert_eq!(output[1].1, 1989);
        assert_eq!(output[1].1, output[2].1);
        assert_eq!(output[1].2, output[2].2);
    }
}
