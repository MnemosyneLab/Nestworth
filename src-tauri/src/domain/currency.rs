use crate::{domain::is_supported_currency, error::AppError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrencyCode([u8; 3]);

impl CurrencyCode {
    pub const CNY: Self = Self(*b"CNY");
    pub const SGD: Self = Self(*b"SGD");
    pub const USD: Self = Self(*b"USD");

    pub fn parse(value: &str) -> Result<Self, AppError> {
        let bytes = value.as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_uppercase) {
            return Err(AppError::validation(
                "currency",
                "Currency must be three uppercase ASCII letters.",
            ));
        }
        Ok(Self([bytes[0], bytes[1], bytes[2]]))
    }

    pub fn parse_supported(value: &str) -> Result<Self, AppError> {
        let currency = Self::parse(value)?;
        if is_supported_currency(currency.as_str()) {
            Ok(currency)
        } else {
            Err(AppError::validation(
                "currency",
                "Currency is not included in the supported catalog.",
            ))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("currency codes are ASCII")
    }
}

impl std::fmt::Display for CurrencyCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::CurrencyCode;

    #[test]
    fn accepts_three_uppercase_letters() {
        assert_eq!(CurrencyCode::parse("CNY").expect("CNY").as_str(), "CNY");
        assert_eq!(CurrencyCode::parse("SGD").expect("SGD"), CurrencyCode::SGD);
        assert_eq!(CurrencyCode::parse("USD").expect("USD"), CurrencyCode::USD);
        assert_eq!(CurrencyCode::parse("JPY").expect("JPY").as_str(), "JPY");
    }

    #[test]
    fn rejects_invalid_currency_codes() {
        assert!(CurrencyCode::parse("cny").is_err());
        assert!(CurrencyCode::parse("CN").is_err());
        assert!(CurrencyCode::parse("CNYY").is_err());
        assert!(CurrencyCode::parse("CN1").is_err());
        assert!(CurrencyCode::parse("").is_err());
        assert!(CurrencyCode::parse(" CNY").is_err());
    }

    #[test]
    fn rejects_well_formed_but_uncatalogued_currency_codes() {
        assert!(CurrencyCode::parse_supported("ZZZ").is_err());
        assert_eq!(
            CurrencyCode::parse_supported("HKD").expect("HKD").as_str(),
            "HKD"
        );
    }

    #[test]
    fn generic_parser_keeps_legacy_currency_codes_readable() {
        assert_eq!(
            CurrencyCode::parse("ZZZ")
                .expect("legacy currency")
                .as_str(),
            "ZZZ"
        );
    }
}
