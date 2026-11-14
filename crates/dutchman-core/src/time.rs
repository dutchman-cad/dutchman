//! A monotonic clock that is safe on every target Dutchman builds for.
//!
//! Natively this is `std::time::Instant`. On `wasm32-unknown-unknown` (the SolidCraft
//! plugin, which has no clock) `std::time::Instant::now()` panics, so there it is a clock
//! that never advances: every elapsed time is zero, and no deadline passes.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use frozen::Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod frozen {
    use std::time::Duration;

    /// A point in time on a clock that never advances.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Instant(Duration);

    impl Instant {
        pub fn now() -> Self {
            Self(Duration::ZERO)
        }
        pub fn elapsed(&self) -> Duration {
            Duration::ZERO
        }
        pub fn duration_since(&self, earlier: Self) -> Duration {
            self.0.saturating_sub(earlier.0)
        }
        pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
            self.0.saturating_sub(earlier.0)
        }
        pub fn checked_add(&self, duration: Duration) -> Option<Self> {
            self.0.checked_add(duration).map(Self)
        }
    }

    impl std::ops::Add<Duration> for Instant {
        type Output = Self;
        fn add(self, duration: Duration) -> Self {
            Self(self.0.saturating_add(duration))
        }
    }

    impl std::ops::Sub<Instant> for Instant {
        type Output = Duration;
        fn sub(self, earlier: Instant) -> Duration {
            self.0.saturating_sub(earlier.0)
        }
    }
}
