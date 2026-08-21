use std::time::Duration;

use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, USER_AGENT,
    },
    Client, StatusCode, Url,
};
use serde_json::Value;

use crate::{
    application::{
        market_data::{
            DailyFxClose, DailyHistoryRequest, DailyInstrumentClose, FxMarketIdentity,
            InstrumentMarketIdentity, MarketDataCapabilities, MarketDataProvider, ProviderFuture,
            YAHOO_FINANCE_PROVIDER,
        },
        providers::{ProviderFxQuote, ProviderInstrument, ProviderQuote},
    },
    domain::{CurrencyCode, FxRate, Timestamp, UnitPrice},
    error::AppError,
};

const HOST: &str = "query1.finance.yahoo.com";
const BASE_PATH: &str = "/v8/finance/chart";
const CURRENT_BODY_LIMIT: usize = 2 * 1024 * 1024;
const HISTORY_BODY_LIMIT: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

const ACCEPT_VALUE: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";
const ACCEPT_ENCODING_VALUE: &str = "gzip, deflate, br, zstd";
const ACCEPT_LANGUAGE_VALUE: &str = "en-US,en;q=0.9";
const PRIORITY_VALUE: &str = "u=0, i";
const SEC_CH_UA_VALUE: &str =
    "\"Not=A?Brand\";v=\"99\", \"Google Chrome\";v=\"151\", \"Chromium\";v=\"151\"";
const SEC_CH_UA_MOBILE_VALUE: &str = "?0";
const SEC_CH_UA_PLATFORM_VALUE: &str = "\"macOS\"";
const SEC_FETCH_DEST_VALUE: &str = "document";
const SEC_FETCH_MODE_VALUE: &str = "navigate";
const SEC_FETCH_SITE_VALUE: &str = "none";
const SEC_FETCH_USER_VALUE: &str = "?1";
const UPGRADE_INSECURE_REQUESTS_VALUE: &str = "1";
const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

#[derive(Clone)]
pub struct YahooChartProvider {
    client: Client,
}

impl YahooChartProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .default_headers(browser_headers())
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("Yahoo HTTP client configuration must be valid");
        Self { client }
    }

    async fn get_chart(
        &self,
        symbol: &str,
        query: &[(&str, String)],
        limit: usize,
    ) -> Result<Value, AppError> {
        let url = chart_url(symbol, query)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| unavailable())?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::ProviderRateLimit);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(AppError::ProviderAuthentication);
        }
        if status == StatusCode::NOT_FOUND {
            return Err(AppError::UnsupportedProviderSymbol {
                message: "The provider does not recognize this symbol.".to_owned(),
            });
        }
        if status.is_server_error() || !status.is_success() {
            return Err(unavailable());
        }
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(AppError::MarketDataResponseTooLarge);
        }
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(AppError::MarketDataResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|_| malformed("The quote provider returned an invalid response."))
    }
}

fn browser_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(ACCEPT_VALUE));
    headers.insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static(ACCEPT_ENCODING_VALUE),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static(ACCEPT_LANGUAGE_VALUE),
    );
    headers.insert(
        HeaderName::from_static("priority"),
        HeaderValue::from_static(PRIORITY_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-ch-ua"),
        HeaderValue::from_static(SEC_CH_UA_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-ch-ua-mobile"),
        HeaderValue::from_static(SEC_CH_UA_MOBILE_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-ch-ua-platform"),
        HeaderValue::from_static(SEC_CH_UA_PLATFORM_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-fetch-dest"),
        HeaderValue::from_static(SEC_FETCH_DEST_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-fetch-mode"),
        HeaderValue::from_static(SEC_FETCH_MODE_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-fetch-site"),
        HeaderValue::from_static(SEC_FETCH_SITE_VALUE),
    );
    headers.insert(
        HeaderName::from_static("sec-fetch-user"),
        HeaderValue::from_static(SEC_FETCH_USER_VALUE),
    );
    headers.insert(
        HeaderName::from_static("upgrade-insecure-requests"),
        HeaderValue::from_static(UPGRADE_INSECURE_REQUESTS_VALUE),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers
}

impl Default for YahooChartProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketDataProvider for YahooChartProvider {
    fn id(&self) -> &'static str {
        YAHOO_FINANCE_PROVIDER
    }

    fn capabilities(&self) -> MarketDataCapabilities {
        MarketDataCapabilities {
            latest_instrument: true,
            latest_fx: true,
            daily_history: true,
            instrument_search: false,
        }
    }

    fn search_instruments(&self, _query: String) -> ProviderFuture<Vec<ProviderInstrument>> {
        Box::pin(async {
            Err(AppError::market_data_unsupported(
                "Instrument search is unavailable.",
            ))
        })
    }

    fn fetch_latest_instrument(
        &self,
        identity: InstrumentMarketIdentity,
    ) -> ProviderFuture<ProviderQuote> {
        let provider = self.clone();
        Box::pin(async move {
            let chart = provider
                .get_chart(
                    &identity.provider_symbol,
                    &[("range", "1mo".to_owned()), ("interval", "1d".to_owned())],
                    CURRENT_BODY_LIMIT,
                )
                .await?;
            normalize_current_instrument(&chart, &identity)
        })
    }

    fn fetch_latest_fx(&self, identity: FxMarketIdentity) -> ProviderFuture<ProviderFxQuote> {
        let provider = self.clone();
        Box::pin(async move {
            let symbol = format!("{}{}=X", identity.base_currency, identity.quote_currency);
            let chart = provider
                .get_chart(
                    &symbol,
                    &[("range", "1mo".to_owned()), ("interval", "1d".to_owned())],
                    CURRENT_BODY_LIMIT,
                )
                .await?;
            normalize_current_fx(&chart, identity)
        })
    }

    fn fetch_daily_instrument(
        &self,
        request: DailyHistoryRequest,
    ) -> ProviderFuture<Vec<DailyInstrumentClose>> {
        let provider = self.clone();
        Box::pin(async move {
            let chart = provider
                .get_chart(
                    &request.provider_symbol,
                    &[
                        ("period1", request.start_at.as_utc().timestamp().to_string()),
                        ("period2", request.end_at.as_utc().timestamp().to_string()),
                        ("interval", "1d".to_owned()),
                    ],
                    HISTORY_BODY_LIMIT,
                )
                .await?;
            normalize_daily_instrument(&chart, &request)
        })
    }

    fn fetch_daily_fx(&self, request: DailyHistoryRequest) -> ProviderFuture<Vec<DailyFxClose>> {
        let provider = self.clone();
        Box::pin(async move {
            let chart = provider
                .get_chart(
                    &request.provider_symbol,
                    &[
                        ("period1", request.start_at.as_utc().timestamp().to_string()),
                        ("period2", request.end_at.as_utc().timestamp().to_string()),
                        ("interval", "1d".to_owned()),
                    ],
                    HISTORY_BODY_LIMIT,
                )
                .await?;
            let (base, quote) = parse_fx_symbol(&request.provider_symbol)?;
            normalize_daily_fx(&chart, &request, base, quote)
        })
    }
}

fn chart_url(symbol: &str, query: &[(&str, String)]) -> Result<Url, AppError> {
    let mut url =
        Url::parse(&format!("https://{HOST}{BASE_PATH}")).map_err(|_| AppError::Internal)?;
    url.path_segments_mut()
        .map_err(|_| AppError::Internal)?
        .push(symbol);
    url.query_pairs_mut()
        .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
    Ok(url)
}

fn parse_fx_symbol(symbol: &str) -> Result<(CurrencyCode, CurrencyCode), AppError> {
    let pair = symbol
        .strip_suffix("=X")
        .filter(|value| value.len() == 6)
        .ok_or_else(|| malformed("The provider FX symbol is invalid."))?;
    let (base, quote) = pair.split_at(3);
    Ok((CurrencyCode::parse(base)?, CurrencyCode::parse(quote)?))
}

fn chart_result(chart: &Value) -> Result<&Value, AppError> {
    if chart
        .pointer("/chart/error")
        .is_some_and(|error| !error.is_null())
    {
        return Err(AppError::UnsupportedProviderSymbol {
            message: "The provider does not recognize this symbol.".to_owned(),
        });
    }
    let results = chart
        .pointer("/chart/result")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("The chart result is invalid."))?;
    if results.len() != 1 {
        return Err(malformed("The chart result cardinality is invalid."));
    }
    Ok(&results[0])
}

fn normalize_current_instrument(
    chart: &Value,
    identity: &InstrumentMarketIdentity,
) -> Result<ProviderQuote, AppError> {
    let result = chart_result(chart)?;
    let currency = response_currency(result)?;
    if currency != identity.expected_currency {
        return Err(malformed(
            "The provider quote currency does not match the target.",
        ));
    }
    let regular_price = result.pointer("/meta/regularMarketPrice");
    let regular_time = result.pointer("/meta/regularMarketTime");
    if regular_price.is_some() || regular_time.is_some() {
        let price =
            number_at(regular_price).ok_or_else(|| malformed("The current price is invalid."))?;
        let timestamp = timestamp_at(regular_time)
            .ok_or_else(|| malformed("The current timestamp is invalid."))?;
        return Ok(ProviderQuote {
            provider_symbol: identity.provider_symbol.clone(),
            unit_price: UnitPrice::parse(&price)?,
            quote_currency: currency,
            quoted_at: timestamp,
            delayed: false,
        });
    }
    let (price, timestamp) = latest_aligned_decimal(result)?;
    Ok(ProviderQuote {
        provider_symbol: identity.provider_symbol.clone(),
        unit_price: UnitPrice::parse(&price)?,
        quote_currency: currency,
        quoted_at: timestamp,
        delayed: true,
    })
}

fn normalize_current_fx(
    chart: &Value,
    identity: FxMarketIdentity,
) -> Result<ProviderFxQuote, AppError> {
    let result = chart_result(chart)?;
    let currency = response_currency(result)?;
    if currency != identity.quote_currency {
        return Err(malformed(
            "The provider FX currency does not match the target.",
        ));
    }
    let regular_price = result.pointer("/meta/regularMarketPrice");
    let regular_time = result.pointer("/meta/regularMarketTime");
    if regular_price.is_some() || regular_time.is_some() {
        let rate =
            number_at(regular_price).ok_or_else(|| malformed("The current FX rate is invalid."))?;
        let timestamp = timestamp_at(regular_time)
            .ok_or_else(|| malformed("The current timestamp is invalid."))?;
        return Ok(ProviderFxQuote {
            base_currency: identity.base_currency,
            quote_currency: currency,
            rate: FxRate::parse(&rate)?,
            quoted_at: timestamp,
            delayed: false,
        });
    }
    let (rate, timestamp) = latest_aligned_decimal(result)?;
    Ok(ProviderFxQuote {
        base_currency: identity.base_currency,
        quote_currency: currency,
        rate: FxRate::parse(&rate)?,
        quoted_at: timestamp,
        delayed: true,
    })
}

fn normalize_daily_instrument(
    chart: &Value,
    request: &DailyHistoryRequest,
) -> Result<Vec<DailyInstrumentClose>, AppError> {
    let result = chart_result(chart)?;
    let currency = response_currency(result)?;
    if currency != request.expected_currency {
        return Err(malformed(
            "The provider quote currency does not match the target.",
        ));
    }
    let (timestamps, closes) = aligned_arrays(result)?;
    let mut output = Vec::new();
    let mut seen = Vec::new();
    for (timestamp, close) in timestamps.iter().zip(closes) {
        let timestamp = timestamp_at(Some(timestamp))
            .ok_or_else(|| malformed("The quote timestamp is invalid."))?;
        if timestamp < request.start_at || timestamp >= request.end_at {
            return Err(malformed(
                "The quote timestamp is outside the requested range.",
            ));
        }
        if close.is_null() {
            continue;
        }
        let value =
            number_at(Some(close)).ok_or_else(|| malformed("The quote close is invalid."))?;
        let unit_price = UnitPrice::parse(&value)?;
        if seen.iter().any(|(at, value): &(Timestamp, String)| {
            *at == timestamp && value != &unit_price.canonical()
        }) {
            return Err(malformed("The provider returned conflicting timestamps."));
        }
        seen.push((timestamp.clone(), unit_price.canonical()));
        output.push(DailyInstrumentClose {
            unit_price,
            quote_currency: currency,
            quoted_at: timestamp,
            delayed: true,
        });
    }
    output.sort_by(|left, right| left.quoted_at.cmp(&right.quoted_at));
    Ok(output)
}

fn normalize_daily_fx(
    chart: &Value,
    request: &DailyHistoryRequest,
    base: CurrencyCode,
    quote: CurrencyCode,
) -> Result<Vec<DailyFxClose>, AppError> {
    let result = chart_result(chart)?;
    if response_currency(result)? != request.expected_currency || quote != request.expected_currency
    {
        return Err(malformed(
            "The provider FX currency does not match the target.",
        ));
    }
    let (timestamps, closes) = aligned_arrays(result)?;
    let mut output = Vec::new();
    let mut seen = Vec::new();
    for (timestamp, close) in timestamps.iter().zip(closes) {
        let timestamp = timestamp_at(Some(timestamp))
            .ok_or_else(|| malformed("The quote timestamp is invalid."))?;
        if timestamp < request.start_at || timestamp >= request.end_at {
            return Err(malformed(
                "The quote timestamp is outside the requested range.",
            ));
        }
        if close.is_null() {
            continue;
        }
        let value = number_at(Some(close)).ok_or_else(|| malformed("The FX close is invalid."))?;
        let rate = FxRate::parse(&value)?;
        if seen
            .iter()
            .any(|(at, value): &(Timestamp, String)| *at == timestamp && value != &rate.canonical())
        {
            return Err(malformed("The provider returned conflicting timestamps."));
        }
        seen.push((timestamp.clone(), rate.canonical()));
        output.push(DailyFxClose {
            rate,
            base_currency: base,
            quote_currency: quote,
            quoted_at: timestamp,
            delayed: true,
        });
    }
    output.sort_by(|left, right| left.quoted_at.cmp(&right.quoted_at));
    Ok(output)
}

fn latest_aligned_decimal(result: &Value) -> Result<(String, Timestamp), AppError> {
    let (timestamps, closes) = aligned_arrays(result)?;
    for (timestamp, close) in timestamps.iter().zip(closes).rev() {
        let timestamp = timestamp_at(Some(timestamp))
            .ok_or_else(|| malformed("The quote timestamp is invalid."))?;
        if close.is_null() {
            continue;
        }
        let value =
            number_at(Some(close)).ok_or_else(|| malformed("The quote close is invalid."))?;
        return Ok((value, timestamp));
    }
    Err(malformed("The provider returned no valid quote."))
}

fn aligned_arrays(result: &Value) -> Result<(&Vec<Value>, &Vec<Value>), AppError> {
    let timestamps = result
        .get("timestamp")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("The quote timestamp array is invalid."))?;
    let closes = result
        .pointer("/indicators/quote/0/close")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("The quote close array is invalid."))?;
    if timestamps.len() != closes.len() {
        return Err(malformed("The quote arrays are not aligned."));
    }
    Ok((timestamps, closes))
}

fn response_currency(result: &Value) -> Result<CurrencyCode, AppError> {
    result
        .pointer("/meta/currency")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("The provider currency is missing."))
        .and_then(CurrencyCode::parse)
}

fn number_at(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_number).map(ToString::to_string)
}

fn timestamp_at(value: Option<&Value>) -> Option<Timestamp> {
    let seconds = value?.as_i64()?;
    chrono::DateTime::from_timestamp(seconds, 0).map(Timestamp::from_utc)
}

fn malformed(message: &str) -> AppError {
    AppError::MalformedProviderResponse {
        message: message.to_owned(),
    }
}

fn unavailable() -> AppError {
    AppError::ProviderUnavailable {
        message: "The quote provider is unavailable.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        browser_headers, normalize_current_instrument, normalize_daily_instrument,
        ACCEPT_ENCODING_VALUE, ACCEPT_LANGUAGE_VALUE, ACCEPT_VALUE, PRIORITY_VALUE,
        SEC_CH_UA_MOBILE_VALUE, SEC_CH_UA_PLATFORM_VALUE, SEC_CH_UA_VALUE, SEC_FETCH_DEST_VALUE,
        SEC_FETCH_MODE_VALUE, SEC_FETCH_SITE_VALUE, SEC_FETCH_USER_VALUE,
        UPGRADE_INSECURE_REQUESTS_VALUE, USER_AGENT_VALUE,
    };
    use crate::{
        application::market_data::{DailyHistoryRequest, InstrumentMarketIdentity},
        domain::{CurrencyCode, Timestamp},
    };

    fn fixture(name: &str) -> serde_json::Value {
        serde_json::from_str(
            &fs::read_to_string(format!("test-fixtures/v0.1.6/{name}.json")).expect("fixture"),
        )
        .expect("json")
    }

    fn identity(symbol: &str) -> InstrumentMarketIdentity {
        InstrumentMarketIdentity {
            provider_id: "yahoo_finance".to_owned(),
            provider_symbol: symbol.to_owned(),
            expected_currency: CurrencyCode::USD,
        }
    }

    #[test]
    fn current_primary_and_fallback_are_exact() {
        let primary = normalize_current_instrument(&fixture("current-primary"), &identity("NVDA"))
            .expect("primary");
        assert_eq!(primary.unit_price.canonical(), "143.25");
        assert!(!primary.delayed);
        let fallback = normalize_current_instrument(&fixture("current-fallback"), &identity("QQQ"))
            .expect("fallback");
        assert_eq!(fallback.unit_price.canonical(), "517.125");
        assert!(fallback.delayed);
    }

    #[test]
    fn daily_null_close_is_skipped_and_values_are_bounded() {
        let request = DailyHistoryRequest {
            provider_id: "yahoo_finance".to_owned(),
            provider_symbol: "QQQ".to_owned(),
            expected_currency: CurrencyCode::USD,
            start_at: Timestamp::parse("2026-08-17T00:00:00Z").expect("start"),
            end_at: Timestamp::parse("2026-08-20T00:00:00Z").expect("end"),
        };
        let values =
            normalize_daily_instrument(&fixture("history-gaps"), &request).expect("history");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].unit_price.canonical(), "515.5");
        assert_eq!(values[1].unit_price.canonical(), "517.125");
    }

    #[test]
    fn malformed_envelopes_write_nothing() {
        for name in [
            "invalid-cardinality",
            "array-mismatch",
            "currency-mismatch",
            "malformed-decimal",
            "unknown-symbol",
        ] {
            let result = normalize_current_instrument(&fixture(name), &identity("NVDA"));
            assert!(result.is_err(), "{name}");
        }
    }

    #[test]
    fn symbols_are_encoded_as_one_path_segment() {
        let url = super::chart_url("^NDX/evil", &[]).expect("url");
        assert_eq!(url.host_str(), Some("query1.finance.yahoo.com"));
        assert!(url.as_str().contains("/v8/finance/chart/^NDX%2Fevil"));
    }

    #[test]
    fn browser_headers_match_the_configured_request_profile() {
        let headers = browser_headers();
        assert_eq!(headers.len(), 13);
        assert_eq!(headers["accept"], ACCEPT_VALUE);
        assert_eq!(headers["accept-encoding"], ACCEPT_ENCODING_VALUE);
        assert_eq!(headers["accept-language"], ACCEPT_LANGUAGE_VALUE);
        assert_eq!(headers["priority"], PRIORITY_VALUE);
        assert_eq!(headers["sec-ch-ua"], SEC_CH_UA_VALUE);
        assert_eq!(headers["sec-ch-ua-mobile"], SEC_CH_UA_MOBILE_VALUE);
        assert_eq!(headers["sec-ch-ua-platform"], SEC_CH_UA_PLATFORM_VALUE);
        assert_eq!(headers["sec-fetch-dest"], SEC_FETCH_DEST_VALUE);
        assert_eq!(headers["sec-fetch-mode"], SEC_FETCH_MODE_VALUE);
        assert_eq!(headers["sec-fetch-site"], SEC_FETCH_SITE_VALUE);
        assert_eq!(headers["sec-fetch-user"], SEC_FETCH_USER_VALUE);
        assert_eq!(
            headers["upgrade-insecure-requests"],
            UPGRADE_INSECURE_REQUESTS_VALUE
        );
        assert_eq!(headers["user-agent"], USER_AGENT_VALUE);
    }
}
