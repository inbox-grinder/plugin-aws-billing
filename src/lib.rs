use plugin_sdk::{export_plugin, EmailInput, ExtractionAction, ExtractionResult};

export_plugin!(process_email);

fn process_email(input: EmailInput) -> ExtractionResult {
    let body = normalize_body(&input);
    let account_suffix = extract_account_suffix(&input.subject, &body);
    let total = extract_total(&body).unwrap_or_else(|| "unknown amount".to_string());
    let card_suffix = extract_after_digits(&body, "credit card ending in ", 4);
    let billing_url = extract_url_containing(&body, "billing/home#/bills");
    let payment_url = extract_url_containing(&body, "billing/home#/paymenthistory");
    let billing_period = billing_url.as_deref().and_then(extract_year_month);

    let mut result = ExtractionResult::new(
        "aws.billing_statement",
        "aws-billing-statement",
        0.97,
        format!("AWS bill {total} for account ending {account_suffix}"),
    );

    result.fields = serde_json::json!({
        "vendor": "Amazon Web Services",
        "accountSuffix": account_suffix,
        "total": total,
        "currency": "USD",
        "defaultPaymentCardSuffix": card_suffix,
        "billingPeriod": billing_period,
    });

    if let Some(url) = billing_url {
        result
            .actions
            .push(ExtractionAction::open_url("open-bill", "Open Bill", url));
    }
    if let Some(url) = payment_url {
        result.actions.push(ExtractionAction::open_url(
            "open-payment-history",
            "Payment History",
            url,
        ));
    }
    result
        .actions
        .push(ExtractionAction::suggest_archive("archive", "Archive"));

    result.automation_hints.push(serde_json::json!({
        "kind": "archive_after",
        "condition": "statement_available",
        "defaultEnabled": false,
        "configKey": "autoArchivePaidStatements"
    }));

    result
}

fn normalize_body(input: &EmailInput) -> String {
    let raw = input
        .text_body
        .as_deref()
        .or(input.html_body.as_deref())
        .unwrap_or("");
    strip_html(raw)
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn extract_account_suffix(subject: &str, body: &str) -> String {
    extract_after_digits(body, "account ending in ****", 4)
        .or_else(|| {
            subject
                .rsplit_once("Account: ")
                .map(|(_, tail)| tail.to_string())
        })
        .map(|value| {
            value
                .chars()
                .filter(|ch| ch.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|digits| {
            if digits.len() >= 4 {
                Some(digits[digits.len() - 4..].to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn extract_total(body: &str) -> Option<String> {
    let marker = "Total: $";
    let start = body.find(marker)? + marker.len();
    let amount = body[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    if amount.is_empty() {
        None
    } else {
        Some(format!("${amount}"))
    }
}

fn extract_after_digits<'a>(body: &'a str, marker: &str, count: usize) -> Option<String> {
    let start = body.find(marker)? + marker.len();
    let digits = body[start..]
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(count)
        .collect::<String>();
    if digits.len() == count {
        Some(digits)
    } else {
        None
    }
}

fn extract_url_containing(body: &str, needle: &str) -> Option<String> {
    body.split_whitespace()
        .map(|part| part.trim_matches(|ch: char| matches!(ch, '.' | ',' | ')' | '(')))
        .find(|part| part.starts_with("https://") && part.contains(needle))
        .map(ToString::to_string)
}

fn extract_year_month(url: &str) -> Option<String> {
    let year = query_value(url, "year")?;
    let month = query_value(url, "month")?;
    Some(format!("{year}-{month:0>2}"))
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = url.find(&marker)? + marker.len();
    Some(
        url[start..]
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_synthetic_billing_fixture() {
        let input =
            plugin_sdk::parse_email_input(include_str!("../fixtures/email-input.json")).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/extraction-result.json")).unwrap();
        assert_eq!(
            serde_json::to_value(process_email(input)).unwrap(),
            expected
        );
    }
}
