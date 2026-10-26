use chrono::{DateTime, LocalResult, TimeZone, Timelike, Utc};
use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use simplelog::*;
use std::collections::HashMap;
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

/// Row as read from the CSV: local (Polish) time, without any timezone info.
#[derive(Deserialize, Debug)]
struct RawEntry {
    #[serde(rename = "Data", deserialize_with = "de_datetime_from_str")]
    date_time: chrono::NaiveDateTime,
    #[serde(rename = " Strefa")]
    tariff: String,
    #[serde(rename = " Wartość ", deserialize_with = "de_float_from_str")]
    kwh_value: f64,
    #[serde(rename = "Rodzaj", deserialize_with = "de_kind_from_str")]
    kind: EnergyKind,
    #[serde(rename = "Status")]
    #[allow(dead_code)] // deserialized from the CSV, but not used (yet)
    status: String,
}

/// Row with an unambiguous point in time (start of the hour slot, UTC).
#[derive(Debug)]
pub struct Entry {
    pub date_time: DateTime<Utc>,
    pub tariff: String,
    pub kwh_value: f64,
    pub kind: EnergyKind,
}

/// Timezone of the Tauron data (CSV contains local time only).
const TAURON_TZ: chrono_tz::Tz = chrono_tz::Europe::Warsaw;

/// Converts local CSV times to UTC instants.
///
/// The CSV has no offset, so on the autumn DST change (CEST -> CET) the
/// repeated local hour appears twice with the same label. The first
/// occurrence (per energy kind) is the summer-time one, the second one the
/// winter-time one. Slots in the non-existing spring hour are skipped.
fn resolve_times(raw: Vec<RawEntry>) -> Vec<Entry> {
    let mut seen: HashMap<(chrono::NaiveDateTime, String), u32> = HashMap::new();
    let mut out = Vec::with_capacity(raw.len());
    for r in raw {
        let n = seen.entry((r.date_time, r.kind.to_string())).or_insert(0);
        *n += 1;
        let occurrence = *n;
        let local = match TAURON_TZ.from_local_datetime(&r.date_time) {
            LocalResult::Single(t) => {
                if occurrence > 1 {
                    warn!(
                        "Duplicated entry for {} ({}) - the later one will overwrite the earlier",
                        r.date_time, r.kind
                    );
                }
                t
            }
            LocalResult::Ambiguous(first, second) => match occurrence {
                1 => first,  //CEST (summer time, UTC+2)
                2 => second, //CET (winter time, UTC+1)
                _ => {
                    warn!(
                        "Skipping entry: {} ({}) appears {} times",
                        r.date_time, r.kind, occurrence
                    );
                    continue;
                }
            },
            LocalResult::None => {
                warn!(
                    "Skipping entry: {} ({}) does not exist in {} (DST gap)",
                    r.date_time, r.kind, TAURON_TZ
                );
                continue;
            }
        };
        out.push(Entry {
            date_time: local.with_timezone(&Utc),
            tariff: r.tariff,
            kwh_value: r.kwh_value,
            kind: r.kind,
        });
    }
    out
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
        "pobór [kWh]" => Ok(EnergyKind::Imported),
        "oddanie [kWh]" => Ok(EnergyKind::Exported),
        "pobrana po zbilansowaniu [kWh]" => Ok(EnergyKind::BalancedImported),
        "oddana po zbilansowaniu [kWh]" => Ok(EnergyKind::BalancedExported),
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
    let raw: Vec<RawEntry> = rdr.deserialize().flatten().collect();
    let entries = resolve_times(raw);
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

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Data; Strefa; Wartość ;Rodzaj;Status;\n";

    fn parse_str(rows: &str) -> Vec<Entry> {
        let csv = format!("{}{}", HEADER, rows);
        parse(&mut csv.as_bytes(), false).unwrap()
    }

    fn utc(e: &Entry) -> String {
        e.date_time.format("%Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn autumn_dst_change_keeps_both_hours() {
        // sample from the real CSV: label 3:00 appears twice (CEST -> CET)
        let e = parse_str(
            "2026-10-25 1:00;T2;0,000;oddana po zbilansowaniu [kWh]; licznik; \n\
             2026-10-25 2:00;T2;0,000;oddana po zbilansowaniu [kWh]; licznik; \n\
             2026-10-25 3:00;T2;1,000;oddana po zbilansowaniu [kWh]; licznik; \n\
             2026-10-25 3:00;T2;2,000;oddana po zbilansowaniu [kWh]; licznik; \n\
             2026-10-25 4:00;T2;0,000;oddana po zbilansowaniu [kWh]; licznik; \n\
             2026-10-25 5:00;T2;0,000;oddana po zbilansowaniu [kWh]; licznik; \n",
        );
        let times: Vec<String> = e.iter().map(utc).collect();
        // slot starts in local time: 00:00 01:00 02:00(CEST) 02:00(CET) 03:00 04:00
        assert_eq!(
            times,
            vec![
                "2026-10-24 22:00",
                "2026-10-24 23:00",
                "2026-10-25 00:00", // 02:00 CEST
                "2026-10-25 01:00", // 02:00 CET
                "2026-10-25 02:00",
                "2026-10-25 03:00"
            ]
        );
        assert_eq!(e[2].kwh_value, 1.0);
        assert_eq!(e[3].kwh_value, 2.0);
        // all instants are unique, so the DB primary key will not collide
        let mut u: Vec<_> = e.iter().map(|x| x.date_time).collect();
        u.dedup();
        assert_eq!(u.len(), 6);
    }

    #[test]
    fn dst_duplicates_are_tracked_per_kind() {
        // CSV may contain several kinds in sequence; each has its own 25-hour series
        let e = parse_str(
            "2026-10-25 3:00;T2;1,0;pobór [kWh]; licznik; \n\
             2026-10-25 3:00;T2;2,0;pobór [kWh]; licznik; \n\
             2026-10-25 3:00;T2;3,0;oddanie [kWh]; licznik; \n\
             2026-10-25 3:00;T2;4,0;oddanie [kWh]; licznik; \n",
        );
        let t: Vec<String> = e.iter().map(utc).collect();
        assert_eq!(
            t,
            vec![
                "2026-10-25 00:00",
                "2026-10-25 01:00",
                "2026-10-25 00:00",
                "2026-10-25 01:00"
            ]
        );
    }

    #[test]
    fn normal_day_summer_and_winter() {
        let e = parse_str(
            "2026-07-01 1:00;T1;0,5;pobór [kWh]; licznik; \n\
             2026-07-01 24:00;T1;0,5;pobór [kWh]; licznik; \n\
             2026-12-01 1:00;T1;0,5;pobór [kWh]; licznik; \n",
        );
        assert_eq!(utc(&e[0]), "2026-06-30 22:00"); // 00:00 CEST
        assert_eq!(utc(&e[1]), "2026-07-01 21:00"); // 23:00 CEST
        assert_eq!(utc(&e[2]), "2026-11-30 23:00"); // 00:00 CET
    }

    #[test]
    fn spring_gap_slot_is_skipped() {
        // 2027-03-28: 02:00-03:00 does not exist; label 3:00 would be that slot
        let e = parse_str(
            "2027-03-28 2:00;T1;0,5;pobór [kWh]; licznik; \n\
             2027-03-28 3:00;T1;0,5;pobór [kWh]; licznik; \n\
             2027-03-28 4:00;T1;0,5;pobór [kWh]; licznik; \n",
        );
        let t: Vec<String> = e.iter().map(utc).collect();
        assert_eq!(t, vec!["2027-03-28 00:00", "2027-03-28 01:00"]); // 01:00 CET, 03:00 CEST
    }
}
