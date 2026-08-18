use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn parse(value: &str) -> Result<Self, AppError> {
        DateTime::parse_from_rfc3339(value)
            .map(|date_time| Self(date_time.with_timezone(&Utc)))
            .map_err(|_| {
                AppError::validation("timestamp", "The timestamp must be a UTC RFC 3339 value.")
            })
    }

    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    #[must_use]
    pub fn is_older_than_hours(&self, now: &Timestamp, hours: i64) -> bool {
        now.0.signed_duration_since(self.0) > chrono::Duration::hours(hours)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalendarDate(NaiveDate);

impl CalendarDate {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        if value.len() != 10 {
            return Err(AppError::validation(
                "date",
                "The date must use YYYY-MM-DD.",
            ));
        }
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| AppError::validation("date", "The date must use YYYY-MM-DD."))
    }

    #[must_use]
    pub fn to_ymd(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{CalendarDate, Timestamp};

    #[test]
    fn generated_timestamp_uses_millis_and_z() {
        let value = Timestamp::now().to_rfc3339();
        assert!(value.ends_with('Z'), "{value}");
        assert_eq!(value.len(), 24, "{value}");
        assert_eq!(value.as_bytes()[10], b'T');
        assert_eq!(value.as_bytes()[19], b'.');
        Timestamp::parse(&value).expect("generated timestamp should parse");
    }

    #[test]
    fn timestamp_parse_rejects_empty_values() {
        assert!(Timestamp::parse("").is_err());
        assert!(Timestamp::parse("2026-08-17").is_err());
    }

    #[test]
    fn calendar_date_round_trips() {
        let date = CalendarDate::parse("2026-08-17").expect("valid calendar date");
        assert_eq!(date.to_ymd(), "2026-08-17");
    }

    #[test]
    fn calendar_date_rejects_non_iso_values() {
        assert!(CalendarDate::parse("2026-8-17").is_err());
        assert!(CalendarDate::parse("2026/08/17").is_err());
        assert!(CalendarDate::parse("2026-02-30").is_err());
        assert!(CalendarDate::parse("2026-08-17T00:00:00Z").is_err());
    }
}
