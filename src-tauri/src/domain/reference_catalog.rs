#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceOption {
    pub value: &'static str,
    pub group: &'static str,
}

pub const CURRENCIES: &[ReferenceOption] = &[
    ReferenceOption {
        value: "CNY",
        group: "core",
    },
    ReferenceOption {
        value: "USD",
        group: "core",
    },
    ReferenceOption {
        value: "HKD",
        group: "core",
    },
    ReferenceOption {
        value: "SGD",
        group: "core",
    },
    ReferenceOption {
        value: "EUR",
        group: "core",
    },
    ReferenceOption {
        value: "JPY",
        group: "core",
    },
    ReferenceOption {
        value: "TWD",
        group: "core",
    },
    ReferenceOption {
        value: "KRW",
        group: "core",
    },
    ReferenceOption {
        value: "GBP",
        group: "core",
    },
    ReferenceOption {
        value: "AUD",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "NZD",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "INR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "IDR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "MYR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "THB",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "VND",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "PHP",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "BND",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "MOP",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "KHR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "LAK",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "MMK",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "BDT",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "PKR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "LKR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "NPR",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "MNT",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "KZT",
        group: "asiaPacific",
    },
    ReferenceOption {
        value: "CHF",
        group: "europe",
    },
    ReferenceOption {
        value: "SEK",
        group: "europe",
    },
    ReferenceOption {
        value: "NOK",
        group: "europe",
    },
    ReferenceOption {
        value: "DKK",
        group: "europe",
    },
    ReferenceOption {
        value: "PLN",
        group: "europe",
    },
    ReferenceOption {
        value: "CZK",
        group: "europe",
    },
    ReferenceOption {
        value: "HUF",
        group: "europe",
    },
    ReferenceOption {
        value: "RON",
        group: "europe",
    },
    ReferenceOption {
        value: "RUB",
        group: "europe",
    },
    ReferenceOption {
        value: "UAH",
        group: "europe",
    },
    ReferenceOption {
        value: "TRY",
        group: "europe",
    },
    ReferenceOption {
        value: "AED",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "SAR",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "ILS",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "QAR",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "KWD",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "ZAR",
        group: "middleEastAfrica",
    },
    ReferenceOption {
        value: "CAD",
        group: "americas",
    },
    ReferenceOption {
        value: "BRL",
        group: "americas",
    },
    ReferenceOption {
        value: "MXN",
        group: "americas",
    },
    ReferenceOption {
        value: "ARS",
        group: "americas",
    },
    ReferenceOption {
        value: "CLP",
        group: "americas",
    },
];

pub const COUNTRIES: &[ReferenceOption] = &[
    ReferenceOption {
        value: "CN",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "HK",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "MO",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "TW",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "JP",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "KR",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "SG",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "MY",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "ID",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "TH",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "VN",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "PH",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "BN",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "KH",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "LA",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "MM",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "IN",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "BD",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "PK",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "LK",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "NP",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "BT",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "MV",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "MN",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "KZ",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "KG",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "UZ",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "AE",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "SA",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "IL",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "TR",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "QA",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "BH",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "KW",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "OM",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "IR",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "IQ",
        group: "asiaMiddleEast",
    },
    ReferenceOption {
        value: "GB",
        group: "europe",
    },
    ReferenceOption {
        value: "DE",
        group: "europe",
    },
    ReferenceOption {
        value: "FR",
        group: "europe",
    },
    ReferenceOption {
        value: "IT",
        group: "europe",
    },
    ReferenceOption {
        value: "ES",
        group: "europe",
    },
    ReferenceOption {
        value: "NL",
        group: "europe",
    },
    ReferenceOption {
        value: "CH",
        group: "europe",
    },
    ReferenceOption {
        value: "SE",
        group: "europe",
    },
    ReferenceOption {
        value: "NO",
        group: "europe",
    },
    ReferenceOption {
        value: "DK",
        group: "europe",
    },
    ReferenceOption {
        value: "FI",
        group: "europe",
    },
    ReferenceOption {
        value: "IE",
        group: "europe",
    },
    ReferenceOption {
        value: "BE",
        group: "europe",
    },
    ReferenceOption {
        value: "AT",
        group: "europe",
    },
    ReferenceOption {
        value: "PT",
        group: "europe",
    },
    ReferenceOption {
        value: "LU",
        group: "europe",
    },
    ReferenceOption {
        value: "PL",
        group: "europe",
    },
    ReferenceOption {
        value: "CZ",
        group: "europe",
    },
    ReferenceOption {
        value: "HU",
        group: "europe",
    },
    ReferenceOption {
        value: "RO",
        group: "europe",
    },
    ReferenceOption {
        value: "RU",
        group: "europe",
    },
    ReferenceOption {
        value: "UA",
        group: "europe",
    },
    ReferenceOption {
        value: "GR",
        group: "europe",
    },
    ReferenceOption {
        value: "US",
        group: "americas",
    },
    ReferenceOption {
        value: "CA",
        group: "americas",
    },
    ReferenceOption {
        value: "MX",
        group: "americas",
    },
    ReferenceOption {
        value: "BR",
        group: "americas",
    },
    ReferenceOption {
        value: "AR",
        group: "americas",
    },
    ReferenceOption {
        value: "CL",
        group: "americas",
    },
    ReferenceOption {
        value: "AU",
        group: "oceania",
    },
    ReferenceOption {
        value: "NZ",
        group: "oceania",
    },
    ReferenceOption {
        value: "ZA",
        group: "africa",
    },
    ReferenceOption {
        value: "EG",
        group: "africa",
    },
];

pub const INSTITUTION_TYPES: &[ReferenceOption] = &[
    ReferenceOption {
        value: "bank",
        group: "financial",
    },
    ReferenceOption {
        value: "digital_bank",
        group: "financial",
    },
    ReferenceOption {
        value: "brokerage",
        group: "financial",
    },
    ReferenceOption {
        value: "internet_platform",
        group: "platform",
    },
    ReferenceOption {
        value: "payment_platform",
        group: "platform",
    },
    ReferenceOption {
        value: "digital_wallet",
        group: "platform",
    },
    ReferenceOption {
        value: "fund_manager",
        group: "financial",
    },
    ReferenceOption {
        value: "insurance",
        group: "financial",
    },
    ReferenceOption {
        value: "pension_provider",
        group: "financial",
    },
    ReferenceOption {
        value: "lender",
        group: "financial",
    },
    ReferenceOption {
        value: "crypto_platform",
        group: "platform",
    },
    ReferenceOption {
        value: "real_estate_platform",
        group: "platform",
    },
    ReferenceOption {
        value: "employer",
        group: "other",
    },
    ReferenceOption {
        value: "government",
        group: "other",
    },
    ReferenceOption {
        value: "other",
        group: "other",
    },
];

pub const GROUP_ICONS: &[&str] = &["wallet", "home", "shield", "briefcase", "heart", "star"];

pub const GROUP_COLORS: &[&str] = &[
    "#2563EB", "#16A34A", "#DC2626", "#D97706", "#7C3AED", "#0F766E",
];

pub const LANGUAGES: &[&str] = &["system", "en", "zh-CN"];
pub const APPEARANCES: &[&str] = &["system", "light", "dark"];

#[must_use]
pub fn is_supported_currency(value: &str) -> bool {
    CURRENCIES.iter().any(|option| option.value == value)
}

#[must_use]
pub fn is_supported_country(value: &str) -> bool {
    COUNTRIES.iter().any(|option| option.value == value)
}

#[must_use]
pub fn is_supported_institution_type(value: &str) -> bool {
    INSTITUTION_TYPES.iter().any(|option| option.value == value)
}

#[must_use]
pub fn is_supported_group_icon(value: &str) -> bool {
    GROUP_ICONS.contains(&value)
}

#[must_use]
pub fn is_supported_group_color(value: &str) -> bool {
    GROUP_COLORS.contains(&value)
}

#[must_use]
pub fn is_supported_language(value: &str) -> bool {
    LANGUAGES.contains(&value)
}

#[must_use]
pub fn is_supported_appearance(value: &str) -> bool {
    APPEARANCES.contains(&value)
}

#[cfg(test)]
mod tests {
    use super::{
        is_supported_country, is_supported_currency, is_supported_group_color,
        is_supported_group_icon, is_supported_institution_type, COUNTRIES, CURRENCIES,
        GROUP_COLORS, GROUP_ICONS, INSTITUTION_TYPES,
    };

    #[test]
    fn curated_catalog_has_expected_sizes_and_core_values() {
        assert_eq!(CURRENCIES.len(), 50);
        assert_eq!(COUNTRIES.len(), 70);
        assert_eq!(INSTITUTION_TYPES.len(), 15);
        assert_eq!(
            CURRENCIES
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            vec![
                "CNY", "USD", "HKD", "SGD", "EUR", "JPY", "TWD", "KRW", "GBP", "AUD", "NZD", "INR",
                "IDR", "MYR", "THB", "VND", "PHP", "BND", "MOP", "KHR", "LAK", "MMK", "BDT", "PKR",
                "LKR", "NPR", "MNT", "KZT", "CHF", "SEK", "NOK", "DKK", "PLN", "CZK", "HUF", "RON",
                "RUB", "UAH", "TRY", "AED", "SAR", "ILS", "QAR", "KWD", "ZAR", "CAD", "BRL", "MXN",
                "ARS", "CLP",
            ]
        );
        assert_eq!(
            COUNTRIES
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            vec![
                "CN", "HK", "MO", "TW", "JP", "KR", "SG", "MY", "ID", "TH", "VN", "PH", "BN", "KH",
                "LA", "MM", "IN", "BD", "PK", "LK", "NP", "BT", "MV", "MN", "KZ", "KG", "UZ", "AE",
                "SA", "IL", "TR", "QA", "BH", "KW", "OM", "IR", "IQ", "GB", "DE", "FR", "IT", "ES",
                "NL", "CH", "SE", "NO", "DK", "FI", "IE", "BE", "AT", "PT", "LU", "PL", "CZ", "HU",
                "RO", "RU", "UA", "GR", "US", "CA", "MX", "BR", "AR", "CL", "AU", "NZ", "ZA", "EG",
            ]
        );
        assert_eq!(
            INSTITUTION_TYPES
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            vec![
                "bank",
                "digital_bank",
                "brokerage",
                "internet_platform",
                "payment_platform",
                "digital_wallet",
                "fund_manager",
                "insurance",
                "pension_provider",
                "lender",
                "crypto_platform",
                "real_estate_platform",
                "employer",
                "government",
                "other",
            ]
        );
        assert_eq!(
            GROUP_ICONS,
            &["wallet", "home", "shield", "briefcase", "heart", "star"]
        );
        assert_eq!(
            GROUP_COLORS,
            &["#2563EB", "#16A34A", "#DC2626", "#D97706", "#7C3AED", "#0F766E"]
        );
    }

    #[test]
    fn unsupported_values_are_not_catalog_values() {
        assert!(!is_supported_currency("ZZZ"));
        assert!(!is_supported_country("ZZ"));
        assert!(!is_supported_institution_type("local_bank"));
        assert!(!is_supported_group_icon("custom"));
        assert!(!is_supported_group_color("#FFFFFF"));
    }
}
