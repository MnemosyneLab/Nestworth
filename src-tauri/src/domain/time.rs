use chrono::{DateTime, MappedLocalTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    #[must_use]
    pub fn from_utc(value: DateTime<Utc>) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_utc(&self) -> DateTime<Utc> {
        self.0
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
        self.0.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
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
    pub fn from_naive_date(value: NaiveDate) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_naive_date(self) -> NaiveDate {
        self.0
    }

    #[must_use]
    pub fn to_ymd(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}

/// Confirmed History Origin IANA timezone such as `America/New_York`, `Asia/Singapore`, or `UTC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryTimezone(Tz);

impl HistoryTimezone {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        value.parse::<Tz>().map(Self).map_err(|_| {
            AppError::validation(
                "timezone",
                "The history timezone must be a valid IANA identifier.",
            )
        })
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.0.name()
    }
}

/// Explicit choice for a local time that occurs twice during a fall-back transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbiguousOffset {
    Earlier,
    Later,
}

impl AmbiguousOffset {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "earlier" => Ok(Self::Earlier),
            "later" => Ok(Self::Later),
            _ => Err(AppError::invalid_activity_time(
                "Ambiguous local time must choose earlier or later.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Earlier => "earlier",
            Self::Later => "later",
        }
    }
}

/// Resolves local `YYYY-MM-DD` + `HH:mm` in a History Origin timezone to UTC.
///
/// Nonexistent daylight-saving times are rejected. Ambiguous times require an
/// explicit earlier/later choice. The returned calendar date is the supplied
/// local date.
pub fn resolve_local_datetime(
    timezone: HistoryTimezone,
    local_date: &str,
    local_time: &str,
    ambiguous_offset: Option<AmbiguousOffset>,
) -> Result<(Timestamp, CalendarDate), AppError> {
    let date = CalendarDate::parse(local_date)
        .map_err(|_| AppError::invalid_activity_time("The local date must use YYYY-MM-DD."))?;
    let time = parse_local_clock_time(local_time)?;
    let naive = date.as_naive_date().and_time(time);
    match timezone.0.from_local_datetime(&naive) {
        MappedLocalTime::None => Err(AppError::invalid_activity_time(
            "This local time does not exist because of a daylight-saving transition.",
        )),
        MappedLocalTime::Single(datetime) => {
            Ok((Timestamp::from_utc(datetime.with_timezone(&Utc)), date))
        }
        MappedLocalTime::Ambiguous(earlier, later) => {
            let datetime = match ambiguous_offset {
                Some(AmbiguousOffset::Earlier) => earlier,
                Some(AmbiguousOffset::Later) => later,
                None => {
                    return Err(AppError::invalid_activity_time(
                        "This local time occurs twice; choose the earlier or later offset.",
                    ));
                }
            };
            Ok((Timestamp::from_utc(datetime.with_timezone(&Utc)), date))
        }
    }
}

/// Resolves local date/time and rejects values before History Origin or in the future.
pub fn resolve_activity_time(
    timezone: HistoryTimezone,
    local_date: &str,
    local_time: &str,
    ambiguous_offset: Option<AmbiguousOffset>,
    origin_at: &Timestamp,
    now: &Timestamp,
) -> Result<(Timestamp, CalendarDate), AppError> {
    let (effective_at, effective_local_date) =
        resolve_local_datetime(timezone, local_date, local_time, ambiguous_offset)?;
    validate_activity_time(&effective_at, origin_at, now)?;
    Ok((effective_at, effective_local_date))
}

pub fn validate_activity_time(
    effective_at: &Timestamp,
    origin_at: &Timestamp,
    now: &Timestamp,
) -> Result<(), AppError> {
    if effective_at < origin_at {
        return Err(AppError::invalid_activity_time(
            "The activity time cannot be before history origin.",
        ));
    }
    if effective_at > now {
        return Err(AppError::invalid_activity_time(
            "The activity time cannot be in the future.",
        ));
    }
    Ok(())
}

fn parse_local_clock_time(value: &str) -> Result<NaiveTime, AppError> {
    if value.len() != 5 {
        return Err(AppError::invalid_activity_time(
            "The local time must use HH:mm.",
        ));
    }
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| AppError::invalid_activity_time("The local time must use HH:mm."))
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_activity_time, resolve_local_datetime, validate_activity_time, AmbiguousOffset,
        CalendarDate, HistoryTimezone, Timestamp,
    };
    use crate::error::AppError;

    fn ny() -> HistoryTimezone {
        HistoryTimezone::parse("America/New_York").expect("IANA timezone")
    }

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

    #[test]
    fn history_timezone_parses_iana_identifiers() {
        assert_eq!(
            HistoryTimezone::parse("America/New_York")
                .expect("ny")
                .as_str(),
            "America/New_York"
        );
        assert_eq!(
            HistoryTimezone::parse("Asia/Singapore")
                .expect("sg")
                .as_str(),
            "Asia/Singapore"
        );
        assert_eq!(HistoryTimezone::parse("UTC").expect("utc").as_str(), "UTC");
        assert!(HistoryTimezone::parse("").is_err());
        assert!(HistoryTimezone::parse("Not/A_Zone").is_err());
        assert!(HistoryTimezone::parse("America/NewYork").is_err());
        assert!(HistoryTimezone::parse("GMT+8").is_err());
    }

    #[test]
    fn dst_nonexistent_local_time_is_rejected() {
        let error = resolve_local_datetime(ny(), "2026-03-08", "02:30", None)
            .expect_err("spring-forward gap");
        assert!(matches!(error, AppError::InvalidActivityTime { .. }));
        assert_eq!(
            error.into_command_error().code,
            crate::error::ErrorCode::InvalidActivityTime
        );
    }

    #[test]
    fn dst_ambiguous_local_time_requires_explicit_offset() {
        let error = resolve_local_datetime(ny(), "2026-11-01", "01:30", None)
            .expect_err("fall-back overlap");
        assert!(matches!(error, AppError::InvalidActivityTime { .. }));
        let earlier =
            resolve_local_datetime(ny(), "2026-11-01", "01:30", Some(AmbiguousOffset::Earlier))
                .expect("earlier");
        let later =
            resolve_local_datetime(ny(), "2026-11-01", "01:30", Some(AmbiguousOffset::Later))
                .expect("later");
        assert_eq!(earlier.0.to_rfc3339(), "2026-11-01T05:30:00.000Z");
        assert_eq!(later.0.to_rfc3339(), "2026-11-01T06:30:00.000Z");
        assert_eq!(earlier.1.to_ymd(), "2026-11-01");
        assert_eq!(later.1.to_ymd(), "2026-11-01");
        assert!(earlier.0 < later.0);
    }

    #[test]
    fn persisted_calendar_date_is_the_local_date() {
        let (timestamp, date) =
            resolve_local_datetime(ny(), "2026-01-01", "23:30", None).expect("evening");
        assert_eq!(date.to_ymd(), "2026-01-01");
        assert_eq!(timestamp.to_rfc3339(), "2026-01-02T04:30:00.000Z");
    }

    #[test]
    fn malformed_local_date_and_time_are_rejected() {
        assert!(matches!(
            resolve_local_datetime(ny(), "2026-1-01", "12:00", None).expect_err("short date"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(matches!(
            resolve_local_datetime(ny(), "2026-02-30", "12:00", None).expect_err("invalid date"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(matches!(
            resolve_local_datetime(ny(), "2026-03-08", "9:30", None).expect_err("short time"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(matches!(
            resolve_local_datetime(ny(), "2026-03-08", "12:00:00", None).expect_err("seconds"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(matches!(
            resolve_local_datetime(ny(), "2026-03-08", "24:00", None).expect_err("24 hour"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(AmbiguousOffset::parse("middle").is_err());
        assert_eq!(
            AmbiguousOffset::parse("earlier").expect("earlier").as_str(),
            "earlier"
        );
        assert_eq!(
            AmbiguousOffset::parse("later").expect("later").as_str(),
            "later"
        );
    }

    #[test]
    fn effective_time_cannot_be_before_origin_or_in_the_future() {
        let origin = Timestamp::parse("2026-01-01T00:00:00.000Z").expect("origin");
        let now = Timestamp::parse("2026-06-01T12:00:00.000Z").expect("now");
        let before = Timestamp::parse("2025-12-31T23:59:59.000Z").expect("before");
        let future = Timestamp::parse("2026-06-01T12:00:00.001Z").expect("future");
        assert!(matches!(
            validate_activity_time(&before, &origin, &now).expect_err("before origin"),
            AppError::InvalidActivityTime { .. }
        ));
        assert!(matches!(
            validate_activity_time(&future, &origin, &now).expect_err("future"),
            AppError::InvalidActivityTime { .. }
        ));
        validate_activity_time(&origin, &origin, &now).expect("origin instant is allowed");
        validate_activity_time(&now, &origin, &now).expect("now is allowed");
        let error = resolve_activity_time(ny(), "2025-12-31", "12:00", None, &origin, &now)
            .expect_err("local time before origin");
        assert!(matches!(error, AppError::InvalidActivityTime { .. }));
        let error = resolve_activity_time(ny(), "2026-06-02", "12:00", None, &origin, &now)
            .expect_err("local time in the future");
        assert!(matches!(error, AppError::InvalidActivityTime { .. }));
        resolve_activity_time(ny(), "2026-06-01", "08:00", None, &origin, &now)
            .expect("08:00 New York is 12:00 UTC");
    }
}
