use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value;
use simplelog::*;
use std::io::BufRead;
use std::str::FromStr;

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct Entry {
    #[serde(deserialize_with = "de_type_from_str")]
    pub EC: f64,
    pub Date: chrono::NaiveDate,
    #[serde(deserialize_with = "de_type_from_str")]
    pub Hour: u8,
    #[serde(deserialize_with = "de_type_from_str")]
    pub Status: u8,
    #[serde(deserialize_with = "de_bool_from_str")]
    pub Extra: bool,
    #[serde(deserialize_with = "de_type_from_str")]
    pub Zone: u8,
    pub ZoneName: String,
    pub Taryfa: Option<String>,
}

fn de_type_from_str<'de, T: FromStr, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    let s = <String>::deserialize(deserializer)?;
    T::from_str(&s).map_err(de::Error::custom)
}

fn de_bool_from_str<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = <String>::deserialize(deserializer)?;
    match s.as_ref() {
        "N" => Ok(false),
        "T" => Ok(true),
        _ => Err(de::Error::custom("Field is not T/N")),
    }
}

pub fn parse_from_reader<R: BufRead>(
    reader: &mut R,
    print_entries: bool,
) -> Result<(Vec<Entry>, Vec<Entry>), Box<dyn std::error::Error>> {
    info!("Loading JSON...");
    let json: Value = serde_json::from_reader(reader)?;
    parse_entries(json, print_entries)
}

pub fn parse_from_string(
    s: String,
    print_entries: bool,
) -> Result<(Vec<Entry>, Vec<Entry>), Box<dyn std::error::Error>> {
    info!("Loading JSON...");
    let json: Value = serde_json::from_str(&s)?;
    parse_entries(json, print_entries)
}

fn parse_entries(
    json: Value,
    print_entries: bool,
) -> Result<(Vec<Entry>, Vec<Entry>), Box<dyn std::error::Error>> {
    info!("Parsing entries...");
    let import = &json["dane"]["chart"];
    let export = &json["dane"]["OZE"];
    let imported: Vec<Entry> = serde_json::from_value(import.clone())?;
    let exported: Vec<Entry> = serde_json::from_value(export.clone())?;
    if print_entries {
        for item in &imported {
            info!("[Energy imported]: {:?}", item);
        }
        for item in &exported {
            info!("[Energy exported]: {:?}", item);
        }
    }
    if imported.is_empty() && exported.is_empty() {
        Err("Error: no entries available!")?
    } else {
        Ok((imported, exported))
    }
}
