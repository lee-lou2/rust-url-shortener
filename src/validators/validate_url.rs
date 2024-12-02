use regex::Regex;

pub fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Err("이메일이 없습니다.".to_string());
    }
    Ok(())
}

pub fn validate_url(url: &str) -> Result<(), String> {
    let url_regex = Regex::new(r"^https?://[^\s/$.?#].[^\s]*$").unwrap();
    if !url_regex.is_match(url) {
        return Err("URL 형태가 올바르지 않습니다.".to_string());
    }
    Ok(())
}

pub fn validate_webhook_url(url: &str) -> Result<(), String> {
    if !url.is_empty() {
        validate_url(url)?;
    }
    Ok(())
}

pub fn validate_fallback_url(url: &str) -> Result<(), String> {
    if !url.is_empty() {
        validate_url(url)?;
    }
    Ok(())
}
