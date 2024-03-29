use chrono::Timelike;
use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use simplelog::*;
use std::fmt;
use std::ops::{Add, Sub};

#[derive(Debug)]
pub enum EnergyKind {
    Imported,
    Exported,
    BalancedImported,
    BalancedExported,
}

impl fmt::Display for EnergyKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EnergyKind::Imported => write!(f, "imported"),
            EnergyKind::Exported => write!(f, "exported"),
            EnergyKind::BalancedImported => write!(f, "balanced_imported"),
            EnergyKind::BalancedExported => write!(f, "balanced_exported"),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Entry {
    #[serde(rename = "Data", deserialize_with = "de_datetime_from_str")]
    pub date_time: chrono::NaiveDateTime,
    #[serde(rename = " Wartość kWh", deserialize_with = "de_float_from_str")]
    pub kwh_value: f64,
    #[serde(rename = "Rodzaj", deserialize_with = "de_kind_from_str")]
    pub kind: EnergyKind,
}

fn de_datetime_from_str<'de, D>(deserializer: D) -> Result<chrono::NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    //Tauron is formatting the hours from 1:00 to 24:00,
    //thus making it `out of range` for automatic parsing
    //Read with a "trick", which is: reading hour as a minute
    //and make a correction afterwards
    chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %M:%H")
        .map_err(serde::de::Error::custom)
        .map(|i| {
            i.add(chrono::Duration::hours((i.minute() - 1).into()))
                .sub(chrono::Duration::minutes((i.minute()).into()))
        })
}

fn de_float_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = <String>::deserialize(deserializer)?;
    //Tauron is formatting values like: "0,774" instead of "0.774"
    //we need to change comma to dot before float parsing
    s.replace(',', ".").parse().map_err(de::Error::custom)
}

fn de_kind_from_str<'de, D>(deserializer: D) -> Result<EnergyKind, D::Error>
where
    D: Deserializer<'de>,
{
    let s = <String>::deserialize(deserializer)?;
    match s.as_ref() {
        "pobór" => Ok(EnergyKind::Imported),
        "oddanie" => Ok(EnergyKind::Exported),
        "pobrana po zbilansowaniu" => Ok(EnergyKind::BalancedImported),
        "oddana po zbilansowaniu" => Ok(EnergyKind::BalancedExported),
        _ => Err(de::Error::custom(
            "Field is not `pobór`/`oddanie`/`pobrana po zbilansowaniu`/`oddana po zbilansowaniu`",
        )),
    }
}

pub fn parse<R: std::io::Read>(
    reader: &mut R,
    print_entries: bool,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    info!("Parsing CSV entries...");
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b';')
        .from_reader(reader);
    let entries: Vec<Entry> = rdr.deserialize().flatten().collect();
    if print_entries {
        for item in &entries {
            info!("{:?}", item);
        }
    }
    if entries.is_empty() {
        Err("Error: no entries available!")?
    } else {
        Ok(entries)
    }
}
