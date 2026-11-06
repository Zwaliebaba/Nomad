//! Time sources for the server shell (Design doc 11 §2, §7; D-050; N2 plan §4.4).
//!
//! The simulation knows only ticks. Real time enters the server here and nowhere else: a
//! **monotonic** clock paces live ticks, and the **wall clock** (UTC) gives the tick the
//! Universe should be at, through its `SeasonClock`. [`SystemClock`] reads the real clocks;
//! [`FakeClock`] is set by hand, so tests drive pacing and catch-up without waiting.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A source of monotonic and wall-clock time that the simulation thread can sleep on.
pub trait Clock: Send + Sync + fmt::Debug + 'static {
    /// Time since an arbitrary fixed origin. Never goes backwards.
    fn monotonic(&self) -> Duration;

    /// The wall clock: UTC milliseconds since the Unix epoch.
    fn utc_ms(&self) -> i64;

    /// Blocks until [`Clock::monotonic`] reaches `deadline` or `alarm` is rung, whichever
    /// comes first. May return early; callers re-check the time.
    fn sleep_until(&self, deadline: Duration, alarm: &Arc<Alarm>);
}

/// Wakes a thread sleeping in [`Clock::sleep_until`]: rung when there is work, such as a
/// shutdown request or, for a [`FakeClock`], a change of time.
#[derive(Debug, Default)]
pub struct Alarm {
    rung: Mutex<bool>,
    changed: Condvar,
}

impl Alarm {
    /// A new alarm, not rung.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Rings the alarm: the sleeper, if any, wakes up.
    pub fn ring(&self) {
        *self.lock() = true;
        self.changed.notify_all();
    }

    /// Clears the alarm. Returns whether it had been rung.
    pub fn reset(&self) -> bool {
        std::mem::take(&mut *self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, bool> {
        // A panic elsewhere cannot leave a bool inconsistent.
        self.rung.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The real clocks: [`Instant`] for monotonic time, [`SystemTime`] for UTC.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose monotonic origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }

    fn utc_ms(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_millis()).map_or(i64::MIN, |ms| -ms),
        }
    }

    fn sleep_until(&self, deadline: Duration, alarm: &Arc<Alarm>) {
        let mut rung = alarm.lock();
        while !*rung {
            let Some(left) = deadline.checked_sub(self.monotonic()) else {
                return;
            };
            if left.is_zero() {
                return;
            }
            rung = alarm
                .changed
                .wait_timeout(rung, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// A clock that moves only when told to, for tests.
///
/// [`FakeClock::advance`] moves monotonic and wall-clock time together, as a running
/// computer does. [`FakeClock::jump_utc`] moves only the wall clock, as when a computer
/// resumes from sleep or its clock is set. Either wakes every thread sleeping on the clock.
#[derive(Clone, Debug)]
pub struct FakeClock {
    inner: Arc<FakeInner>,
}

#[derive(Debug)]
struct FakeInner {
    now: Mutex<FakeNow>,
    sleepers: Mutex<Vec<Weak<Alarm>>>,
}

#[derive(Clone, Copy, Debug)]
struct FakeNow {
    monotonic: Duration,
    utc_ms: i64,
}

impl FakeClock {
    /// A clock at wall-clock time `utc_ms`, with monotonic time zero.
    pub fn new(utc_ms: i64) -> Self {
        Self {
            inner: Arc::new(FakeInner {
                now: Mutex::new(FakeNow {
                    monotonic: Duration::ZERO,
                    utc_ms,
                }),
                sleepers: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Moves monotonic and wall-clock time forward by `by`.
    pub fn advance(&self, by: Duration) {
        {
            let mut now = self.now();
            now.monotonic = now.monotonic.saturating_add(by);
            let ms = i64::try_from(by.as_millis()).unwrap_or(i64::MAX);
            now.utc_ms = now.utc_ms.saturating_add(ms);
        }
        self.wake_sleepers();
    }

    /// Moves only the wall clock, by `by_ms` (negative moves it back).
    pub fn jump_utc(&self, by_ms: i64) {
        {
            let mut now = self.now();
            now.utc_ms = now.utc_ms.saturating_add(by_ms);
        }
        self.wake_sleepers();
    }

    fn now(&self) -> MutexGuard<'_, FakeNow> {
        self.inner
            .now
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn sleepers(&self) -> MutexGuard<'_, Vec<Weak<Alarm>>> {
        self.inner
            .sleepers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn register(&self, alarm: &Arc<Alarm>) {
        let mut sleepers = self.sleepers();
        sleepers.retain(|weak| weak.strong_count() > 0);
        let known = sleepers
            .iter()
            .any(|weak| std::ptr::eq(weak.as_ptr(), Arc::as_ptr(alarm)));
        if !known {
            sleepers.push(Arc::downgrade(alarm));
        }
    }

    fn wake_sleepers(&self) {
        let alarms: Vec<Arc<Alarm>> = self.sleepers().iter().filter_map(Weak::upgrade).collect();
        for alarm in alarms {
            alarm.ring();
        }
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> Duration {
        self.now().monotonic
    }

    fn utc_ms(&self) -> i64 {
        self.now().utc_ms
    }

    fn sleep_until(&self, deadline: Duration, alarm: &Arc<Alarm>) {
        self.register(alarm);
        // The time is checked under the alarm's lock, and `advance` rings the alarm after
        // changing the time, so a change cannot slip in between the check and the wait.
        let mut rung = alarm.lock();
        while !*rung && self.monotonic() < deadline {
            rung = alarm
                .changed
                .wait(rung)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn fake_clock_moves_only_when_told() {
        let clock = FakeClock::new(1_000);
        assert_eq!(clock.monotonic(), Duration::ZERO);
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.monotonic(), Duration::from_millis(250));
        assert_eq!(clock.utc_ms(), 1_250);
        clock.jump_utc(-50);
        assert_eq!(clock.monotonic(), Duration::from_millis(250));
        assert_eq!(clock.utc_ms(), 1_200);
    }

    #[test]
    fn fake_sleep_returns_once_the_deadline_passes() {
        let clock = FakeClock::new(0);
        let alarm = Alarm::new();
        let sleeper = {
            let clock = clock.clone();
            let alarm = Arc::clone(&alarm);
            thread::spawn(move || clock.sleep_until(Duration::from_secs(1), &alarm))
        };
        // Whether the sleeper is already waiting or not, advancing past the deadline wakes it.
        clock.advance(Duration::from_secs(2));
        sleeper.join().unwrap();
    }

    #[test]
    fn ringing_the_alarm_ends_a_sleep() {
        for clock in [
            Arc::new(FakeClock::new(0)) as Arc<dyn Clock>,
            Arc::new(SystemClock::new()),
        ] {
            let alarm = Alarm::new();
            let sleeper = {
                let alarm = Arc::clone(&alarm);
                let clock = Arc::clone(&clock);
                thread::spawn(move || clock.sleep_until(Duration::from_hours(1), &alarm))
            };
            alarm.ring();
            sleeper.join().unwrap();
            assert!(alarm.reset());
            assert!(!alarm.reset());
        }
    }

    #[test]
    fn system_sleep_with_a_past_deadline_returns_at_once() {
        let clock = SystemClock::new();
        clock.sleep_until(Duration::ZERO, &Alarm::new());
        assert!(clock.utc_ms() > 1_700_000_000_000);
    }
}
