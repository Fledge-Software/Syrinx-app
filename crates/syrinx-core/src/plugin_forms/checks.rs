//! The checks the host makes itself before a form's values reach its plugin (#29): what
//! `required` and each `forms.field-kind` imply. Formats a kind leaves open, such as an email
//! address or a date, are the plugin's to judge in `forms.validate`.

use super::{ChoiceOption, Field, FieldError, FieldKind, FieldValue, FormSpec, NumberOptions};

/// Every field that fails the host's checks, in the form's order. A field missing from
/// `values` counts as empty.
pub fn host_checks(spec: &FormSpec, values: &[FieldValue]) -> Vec<FieldError> {
    spec.fields
        .iter()
        .filter_map(|field| {
            let value = values
                .iter()
                .find(|value| value.name == field.name)
                .map_or("", |value| value.value.as_str());
            let message = field_problem(field, value)?;
            Some(FieldError {
                name: field.name.clone(),
                message,
            })
        })
        .collect()
}

/// What is wrong with `value` for `field`, if anything. An empty optional field is fine; one of
/// only spaces is sent as typed, so it is checked like any other value.
fn field_problem(field: &Field, value: &str) -> Option<String> {
    if field.required && value.trim().is_empty() {
        return Some("Required.".to_owned());
    }
    if value.is_empty() {
        return None;
    }
    match &field.kind {
        FieldKind::Text(options) | FieldKind::Multiline(options) => {
            too_long(value, options.max_length)
        }
        FieldKind::Number(options) => number_problem(value, options),
        FieldKind::Money(_) => parse_number(value).err(),
        FieldKind::Choice(options) => not_a_choice(value, options),
        FieldKind::Toggle => not_a_toggle(value),
        FieldKind::Date | FieldKind::DateTime => None,
    }
}

/// Text longer than `max_length` characters.
fn too_long(value: &str, max_length: u32) -> Option<String> {
    // Counted as u64, which holds any character count and any `u32` limit.
    let length = value.chars().count() as u64;
    (length > u64::from(max_length)).then(|| format!("At most {max_length} characters."))
}

/// A number that does not parse, or lies outside the field's range.
fn number_problem(value: &str, options: &NumberOptions) -> Option<String> {
    let number = match parse_number(value) {
        Ok(number) => number,
        Err(problem) => return Some(problem),
    };
    if let Some(min) = options.min
        && number < min
    {
        return Some(format!("At least {min}."));
    }
    if let Some(max) = options.max
        && number > max
    {
        return Some(format!("At most {max}."));
    }
    None
}

/// A finite number, written plainly; a space around it is sent too, so it is refused.
fn parse_number(value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err("Enter a number.".to_owned()),
    }
}

/// A value that is not one of the field's options.
fn not_a_choice(value: &str, options: &[ChoiceOption]) -> Option<String> {
    let listed = options.iter().any(|option| option.value == value);
    (!listed).then(|| "Choose one of the options.".to_owned())
}

/// A toggle holds `true` or `false`.
fn not_a_toggle(value: &str) -> Option<String> {
    let valid = value == "true" || value == "false";
    (!valid).then(|| "Choose on or off.".to_owned())
}
