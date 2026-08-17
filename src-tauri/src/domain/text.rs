use crate::error::AppError;

pub const NAME_MAX_CHARS: usize = 80;
pub const NOTE_MAX_CHARS: usize = 2000;

pub fn parse_name(value: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if !(1..=NAME_MAX_CHARS).contains(&length) {
        return Err(AppError::validation(
            "name",
            "Name must be between 1 and 80 characters.",
        ));
    }
    Ok(trimmed.to_owned())
}

pub fn parse_optional_text(
    value: Option<&str>,
    max_chars: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > max_chars {
        return Err(AppError::validation(
            field,
            &format!("Must be at most {max_chars} characters."),
        ));
    }
    Ok(Some(trimmed.to_owned()))
}

pub fn parse_optional_note(value: Option<&str>) -> Result<Option<String>, AppError> {
    parse_optional_text(value, NOTE_MAX_CHARS, "note")
}

#[cfg(test)]
mod tests {
    use super::{parse_name, parse_optional_note};

    #[test]
    fn name_trims_and_accepts_unicode() {
        assert_eq!(parse_name("  王家  ").expect("name"), "王家");
    }

    #[test]
    fn name_rejects_empty_and_too_long_values() {
        assert!(parse_name("").is_err());
        assert!(parse_name("   ").is_err());
        let too_long: String = "a".repeat(81);
        assert!(parse_name(&too_long).is_err());
        let max: String = "a".repeat(80);
        assert_eq!(parse_name(&max).expect("max name"), max);
    }

    #[test]
    fn note_rejects_values_over_2000_characters() {
        let too_long: String = "n".repeat(2001);
        assert!(parse_optional_note(Some(&too_long)).is_err());
        assert_eq!(parse_optional_note(Some("  ")).expect("blank note"), None);
    }
}
