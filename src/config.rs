//! TOML configuration.
//!
//! Tip: passwords with special characters are easiest to write as TOML
//! *literal strings* (single quotes) - nothing inside is interpreted:
//!     password = ':_#^abC"!d.EFGHIjk#L;Mn,;op8'
//! A password containing `'` can use `'''...'''`, or a basic string with
//! `\"` / `\\` escaping.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub tauron: Option<TauronConfig>,
    pub postgres: Option<PostgresConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TauronConfig {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresConfig {
    pub host: String,
    pub dbname: String,
    pub username: String,
    pub password: String,
}

impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Config, String> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| e.to_string())?;
        Config::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| {
            // Do not use `e.to_string()`: it prints the offending source line,
            // which may contain a password. Report only line number + message.
            let line = e
                .span()
                .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
            match line {
                Some(l) => format!("line {}: {}", l, e.message()),
                None => e.message().to_string(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = ":_#^abC\"!d.EFGHIjk#L;Mn,;op8";

    #[test]
    fn literal_string_password() {
        let txt = format!(
            "[tauron]\nusername='a@b.pl'\npassword='{}'\n\n[postgres]\nhost='h'\ndbname='d'\nusername='dbuser'\npassword='dbpass'\n",
            PASS
        );
        let c = Config::parse(&txt).unwrap();
        let t = c.tauron.unwrap();
        let p = c.postgres.unwrap();
        assert_eq!(t.username, "a@b.pl");
        assert_eq!(t.password, PASS);
        assert_eq!(p.username, "dbuser");
        assert_eq!(p.password, "dbpass");
    }

    #[test]
    fn basic_string_with_escapes() {
        let txt = "[tauron]\nusername='a'\npassword=\":_#^abC\\\"!d.EFGHIjk#L;Mn,;op8\"\n";
        let c = Config::parse(txt).unwrap();
        assert_eq!(c.tauron.unwrap().password, PASS);
    }

    #[test]
    fn missing_sections_are_none() {
        let c = Config::parse("").unwrap();
        assert!(c.tauron.is_none() && c.postgres.is_none());
    }

    #[test]
    fn errors_do_not_leak_secrets() {
        // unescaped quote inside a basic string
        let e = Config::parse("[tauron]\nusername='a'\npassword=\"sekret\"pass\"\n").unwrap_err();
        assert!(!e.contains("sekret"), "{}", e);
        // missing field / unknown field
        let e = Config::parse("[tauron]\nusername='a'\n").unwrap_err();
        assert!(e.contains("password"), "{}", e);
        assert!(Config::parse("[tauron]\nusername='a'\npassword='b'\nfoo='sekret'\n").is_err());
    }
}
