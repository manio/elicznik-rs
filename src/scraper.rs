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

        client
            .post(&form_action)
            .form(&login_payload)
            .send()
            .await?;

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
        let t = res.text().await?;

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

fn elapsed_ms(elapsed: &std::time::Duration) -> u64 {
    (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64
}
