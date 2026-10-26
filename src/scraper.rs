use simplelog::*;
use std::collections::HashMap;
use std::time::Instant;

static LOGIN_URL: &str = "https://logowanie.tauron-dystrybucja.pl/login";
static DATA_URL: &str = "https://elicznik.tauron-dystrybucja.pl/energia/do/dane";
static SERVICE: &str = "https://elicznik.tauron-dystrybucja.pl";

pub struct Scraper {
    pub name: String,
    pub username: String,
    pub password: String,
    pub start_date: String,
    pub end_date: String,
}

impl Scraper {
    pub async fn get_data(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Chart data parameters
        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("form[from]", &self.start_date);
        params.insert("form[to]", &self.end_date);
        params.insert("form[type]", "godzin");
        params.insert("form[energy][consum]", "1");
        params.insert("form[energy][oze]", "1");
        params.insert("form[energy][netto]", "1");
        params.insert("form[energy][netto_oze]", "1");
        params.insert("form[fileType]", "CSV");

        // Creating client with cookie store
        let client = reqwest::Client::builder().cookie_store(true).build()?;

        let started = Instant::now();

        // Step 1: GET login page with service parameter to get Keycloak form
        info!("{}: 🔒 Session start, opening login page...", self.name);
        let login_url_with_service = format!("{}?service={}", LOGIN_URL, SERVICE);
        let r1 = client.get(&login_url_with_service).send().await?;

        let elapsed = started.elapsed();
        let ms = elapsed_ms(&elapsed);
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        // Step 2: Extract Keycloak form action URL
        let body = r1.text().await?;
        let form_action = extract_keycloak_form_action(&body).ok_or_else(|| {
            error!(
                "{}: ❌ Keycloak login form not found in response",
                self.name
            );
            "Keycloak login form not found"
        })?;

        info!(
            "{}: 🔑 Logging in... (username: <i><green>{}</>)",
            self.name, self.username
        );

        // Step 3: POST credentials to Keycloak form action URL
        let mut sub_started = Instant::now();
        let mut login_payload = HashMap::new();
        login_payload.insert("username", self.username.as_str());
        login_payload.insert("password", self.password.as_str());
        login_payload.insert("credentialId", "");

        let login_res = client
            .post(&form_action)
            .form(&login_payload)
            .send()
            .await?;
        let login_status = login_res.status();
        let login_final_url = login_res.url().clone();
        let login_body = login_res.text().await?;
        debug!(
            "{}: login response: status {}, final url: {}",
            self.name, login_status, login_final_url
        );

        // Keycloak answers a rejected login with the login form again (HTTP 200)
        if !login_status.is_success() {
            return Err(format!("login failed: unexpected HTTP status {}", login_status).into());
        }
        if extract_keycloak_form_action(&login_body).is_some() {
            return Err("login failed: credentials rejected by Tauron \
                        (login form returned again) - check username/password in the config"
                .into());
        }

        let elapsed = sub_started.elapsed();
        let ms = elapsed_ms(&elapsed);
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        // Fetch CSV data
        info!(
            "{}: Requesting CSV data, start date: <b><cyan>{}</>, end date: <b><cyan>{:?}</>",
            self.name, self.start_date, self.end_date
        );
        sub_started = Instant::now();

        let url = reqwest::Url::parse_with_params(DATA_URL, &params)?;
        let res = client.get(url).send().await?;
        let data_status = res.status();
        let data_final_url = res.url().clone();
        let content_type = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        let t = res.text().await?;
        debug!(
            "{}: data response: status {}, content-type: {:?}, final url: {}",
            self.name, data_status, content_type, data_final_url
        );
        if let Err(e) = validate_csv_response(data_status.is_success(), content_type.as_deref(), &t)
        {
            return Err(format!(
                "{} (http status {}, url: {})",
                e, data_status, data_final_url
            )
            .into());
        }

        let elapsed = sub_started.elapsed();
        let ms = elapsed_ms(&elapsed);
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        let elapsed = started.elapsed();
        let ms = elapsed_ms(&elapsed);
        info!(
            "{}: ⌛ total scraping time: <magenta>{}</> ms",
            self.name, ms
        );

        Ok(t)
    }
}

/// Extracts the Keycloak form action URL from the login page HTML.
/// Looks for: <form ... id="kc-form-login" ... action="...">
/// Returns the unescaped action URL, or None if not found.
fn extract_keycloak_form_action(html: &str) -> Option<String> {
    // Find the kc-form-login form tag
    let form_start = html.find(r#"id="kc-form-login""#)?;

    // Search backwards for the opening <form to get the full tag
    let before = &html[..form_start];
    let tag_start = before.rfind("<form")?;
    let tag_end = html[tag_start..].find('>')? + tag_start;
    let form_tag = &html[tag_start..=tag_end];

    // Extract action="..." from the form tag
    let action_start = form_tag.find(r#"action=""#)? + r#"action=""#.len();
    let action_end = form_tag[action_start..].find('"')? + action_start;
    let raw_action = &form_tag[action_start..action_end];

    // Unescape HTML entities (&amp; → &)
    Some(raw_action.replace("&amp;", "&"))
}

/// Checks that the data endpoint returned CSV and not an HTML page
/// (login form, logout/redirect page, error page...), which happens when
/// the session is not authenticated.
fn validate_csv_response(
    http_ok: bool,
    content_type: Option<&str>,
    body: &str,
) -> Result<(), String> {
    if !http_ok {
        return Err("data request failed".to_string());
    }
    let looks_like_html = {
        let head = body.trim_start_matches('\u{feff}').trim_start();
        let lower: String = head.chars().take(64).collect::<String>().to_lowercase();
        lower.starts_with('<')
    };
    let ct_is_html = content_type
        .map(|c| c.to_lowercase().contains("html"))
        .unwrap_or(false);
    if looks_like_html || ct_is_html {
        return Err("Tauron returned an HTML page instead of CSV data - \
                    not logged in (bad credentials or expired session?)"
            .to_string());
    }
    if body.trim().is_empty() {
        return Err("Tauron returned an empty response".to_string());
    }
    Ok(())
}

fn elapsed_ms(elapsed: &std::time::Duration) -> u64 {
    (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_instead_of_csv_is_rejected() {
        // start of the real page returned when not authenticated (logout/redirect page)
        let html = "\n<!DOCTYPE html>\n<html class=\"login-pf\">\n<head>";
        assert!(validate_csv_response(true, Some("text/html;charset=utf-8"), html).is_err());
        assert!(validate_csv_response(true, None, html).is_err());
        assert!(validate_csv_response(true, Some("text/csv"), "<html>").is_err());
        assert!(validate_csv_response(true, Some("text/html"), "a;b\n1;2\n").is_err());
    }

    #[test]
    fn csv_is_accepted() {
        let csv = "Data;Wartosc\n2026-10-01;1.2\n";
        assert!(validate_csv_response(true, Some("text/csv"), csv).is_ok());
        assert!(validate_csv_response(true, None, csv).is_ok());
    }

    #[test]
    fn http_error_and_empty_are_rejected() {
        assert!(validate_csv_response(false, Some("text/csv"), "a;b").is_err());
        assert!(validate_csv_response(true, Some("text/csv"), "  \n").is_err());
    }

    #[test]
    fn login_form_detection() {
        let form = r#"<form id="kc-form-login" action="https://x/y?a=1&amp;b=2" method="post">"#;
        assert_eq!(
            extract_keycloak_form_action(form).as_deref(),
            Some("https://x/y?a=1&b=2")
        );
        assert!(extract_keycloak_form_action("<html>logout</html>").is_none());
    }
}
