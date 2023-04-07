use simplelog::*;
use std::collections::HashMap;
use std::time::Instant;

static URL: &str = "https://logowanie.tauron-dystrybucja.pl/login";
static CHART_URL: &str = "https://elicznik.tauron-dystrybucja.pl/index/charts";
static SERVICE: &str = "https://elicznik.tauron-dystrybucja.pl";

pub struct Scraper {
    pub name: String,
    pub username: String,
    pub password: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

impl Scraper {
    pub async fn get_json_data(&self) -> Result<String, Box<dyn std::error::Error>> {
        //login parameters
        let mut payload = HashMap::new();
        payload.insert("username", &self.username);
        payload.insert("password", &self.password);
        let service = String::from(SERVICE);
        payload.insert("service", &service);

        //chart data parameters
        let mut params = HashMap::new();
        params.insert("dane[paramType]", "csv");
        params.insert("dane[trybCSV]", "godzin");
        params.insert("dane[startDay]", &self.start_date);
        if let Some(date) = &self.end_date {
            params.insert("dane[endDay]", &date);
        }
        params.insert("dane[checkOZE]", "on");

        //creating client with cookie store
        let client = reqwest::Client::builder().cookie_store(true).build()?;

        let started = Instant::now();
        info!("{}: 🔒 Session start, opening login page...", self.name);
        client.get(URL).send().await?;
        let elapsed = started.elapsed();
        let ms = (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64;
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        info!(
            "{}: 🔑 Logging in... (username: <i><green>{}</>)",
            self.name, self.username
        );
        let mut sub_started = Instant::now();
        client.post(URL).form(&payload).send().await?;
        let elapsed = sub_started.elapsed();
        let ms = (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64;
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        info!(
            "{}: Requesting JSON data, start date: <b><cyan>{}</>, end date: <b><cyan>{:?}</>",
            self.name, self.start_date, self.end_date
        );
        sub_started = Instant::now();
        let res = client.post(CHART_URL).form(&params).send().await?;
        let t = res.text().await?;
        let elapsed = sub_started.elapsed();
        let ms = (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64;
        info!(
            "{}: <black>- - -</> ⏱️ response time: <magenta>{}</> ms",
            self.name, ms
        );

        let elapsed = started.elapsed();
        let ms = (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64;
        info!(
            "{}: ⌛ total scraping time: <magenta>{}</> ms",
            self.name, ms
        );
        Ok(t)
    }
}
