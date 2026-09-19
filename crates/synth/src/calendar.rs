//! Dates and timestamps, without a date crate.

use serde::{Deserialize, Serialize};

/// A civil date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Date {
    /// Year.
    pub year: i32,
    /// Month, 1..=12.
    pub month: u32,
    /// Day of month, 1..=31.
    pub day: u32,
}

impl Date {
    /// A date.
    pub const fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Days since 1970-01-01. Howard Hinnant's `days_from_civil`.
    pub fn to_days(self) -> i64 {
        let y = if self.month <= 2 { self.year - 1 } else { self.year } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.month as i64;
        let d = self.day as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01. The inverse of [`Date::to_days`].
    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        Date { year: (if m <= 2 { y + 1 } else { y }) as i32, month: m as u32, day: d as u32 }
    }

    /// This date plus `n` days.
    pub fn plus_days(self, n: i64) -> Self {
        Date::from_days(self.to_days() + n)
    }

    /// 0 = Monday .. 6 = Sunday.
    pub fn weekday(self) -> u32 {
        (self.to_days().rem_euclid(7) as u32 + 3) % 7
    }

    /// Whether this is a Saturday or Sunday.
    pub fn is_weekend(self) -> bool {
        self.weekday() >= 5
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A point in the simulation: a day index and an hour of that day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    /// Day index from the start of the simulation.
    pub day: u32,
    /// Hour of day, 0..=23.
    pub hour: u32,
}

impl Stamp {
    /// A stamp.
    pub const fn new(day: u32, hour: u32) -> Self {
        Self { day, hour }
    }

    /// Hours since the start of the simulation.
    pub fn index(&self) -> u32 {
        self.day * 24 + self.hour
    }

    /// The stamp `hours` after the start of the simulation.
    pub fn from_index(hours: u32) -> Self {
        Stamp::new(hours / 24, hours % 24)
    }

    /// Hours from this stamp to `other`; negative if `other` is in the past.
    pub fn hours_to(&self, other: Stamp) -> f64 {
        other.index() as f64 - self.index() as f64
    }

    /// This stamp rendered against a simulation start date.
    pub fn on(&self, start: Date) -> String {
        format!("{} {:02}:00", start.plus_days(self.day as i64), self.hour)
    }
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "d{:03} {:02}:00", self.day, self.hour)
    }
}
