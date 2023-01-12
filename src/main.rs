use chrono::Local;
use clap::Parser;
use ini::Ini;
use simplelog::*;
use std::fs::File;
use std::io::BufReader;

mod database;
mod parser;
mod scraper;

use crate::database::Database;
use crate::scraper::Scraper;
use std::path::{Path, PathBuf};

/// Simple program to fetch and process `Tauron eLicznik` JSON data.
/// If none arguments are given, it is fetching last two days of data
/// and updates missing values in the configured PostgreSQL database
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Start date in format: YYYY-MM-DD [default: two days ago]
    #[clap(short, long)]
    start: Option<chrono::NaiveDate>,

    /// End date in format: YYYY-MM-DD [default: current date]
    #[clap(short, long)]
    end: Option<chrono::NaiveDate>,

    /// Enable debug info
    #[clap(short, long)]
    debug: bool,

    /// Print all JSON entries
    #[clap(short, long)]
    print: bool,

    /// Input JSON file to read instead of using `Tauron eLicznik`
    #[clap(short, long, parse(from_os_str))]
    input: Option<std::path::PathBuf>,

    /// Output JSON file to write output data (database will be also updated, if configured)
    #[clap(short, long, parse(from_os_str))]
    output: Option<std::path::PathBuf>,

    /// Config file path
    #[clap(short, long, parse(from_os_str), default_value = "/etc/elicznik.conf")]
    config: std::path::PathBuf,
}

fn logging_init(debug: bool) {
    let conf = ConfigBuilder::new()
        .set_time_format("%F, %H:%M:%S%.3f".to_string())
        .set_write_log_enable_colors(true)
        .build();

    let mut loggers = vec![];

    let console_logger: Box<dyn SharedLogger> = TermLogger::new(
        if debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        },
        conf.clone(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );
    loggers.push(console_logger);

    CombinedLogger::init(loggers).expect("Cannot initialize logging subsystem");
}

fn config_read_postgres(conf: Ini) -> Result<Database, Box<dyn std::error::Error>> {
    match conf.section(Some("postgres".to_owned())) {
        Some(section) => Ok(Database {
            name: "🦏 postgres".to_string(),
            host: section.get("host").ok_or("`host` is missing")?.to_string(),
            dbname: section.get("dbname").ok_or("missing `dbname`")?.to_string(),
            username: section
                .get("username")
                .ok_or("missing `username`")?
                .to_string(),
            password: section
                .get("password")
                .ok_or("missing `password`")?
                .to_string(),
        }),
        None => Err("missing [postgres] config section")?,
    }
}

fn config_read_tauron(
    conf: Ini,
    start_date: String,
    end_date: String,
) -> Result<Scraper, Box<dyn std::error::Error>> {
    match conf.section(Some("tauron".to_owned())) {
        Some(section) => Ok(Scraper {
            name: "💡 Tauron eLicznik".to_string(),
            username: section
                .get("username")
                .ok_or("missing `username`")?
                .to_string(),
            password: section
                .get("password")
                .ok_or("missing `password`")?
                .to_string(),
            start_date,
            end_date,
        }),
        None => Err("missing [tauron] config section")?,
    }
}

fn change_file_name(path: impl AsRef<Path>, name: &str) -> PathBuf {
    let path = path.as_ref();
    let mut result = path.to_owned();
    result.set_file_name(name);
    if let Some(ext) = path.extension() {
        result.set_extension(ext);
    }
    result
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    logging_init(args.debug);
    info!("<b><green>elicznik</> started");

    //check date argument consistency
    if args.end.is_some() {
        if args.start.is_none() {
            error!("you cannot pass `end` date parameter without `start` date");
            return Ok(());
        } else if args.end.unwrap() < args.start.unwrap() {
            error!("`end` date is earlier then `start`");
            return Ok(());
        }
    }

    info!("Using config file: <b><blue>{:?}</>", args.config);
    let conf = match Ini::load_from_file(args.config) {
        Ok(c) => c,
        Err(e) => {
            error!("Cannot open config file: {}", e);
            return Ok(());
        }
    };

    //obtain the input data
    let entries = match args.input {
        Some(ref filename) => {
            let mut imported = Ok(vec![]);
            let mut exported = Ok(vec![]);
            for type_ in &["consum", "oze"] {
                let filename = change_file_name(
                    filename,
                    &format!(
                        "{}_{}",
                        filename
                            .file_stem()
                            .unwrap()
                            .to_os_string()
                            .into_string()
                            .unwrap(),
                        type_
                    ),
                );
                let res = match File::open(filename.clone()) {
                    Ok(file) => {
                        info!(
                            "💾 Loading `{}` data from JSON input file: {:?}",
                            type_, &filename
                        );
                        let mut reader = BufReader::new(file);
                        parser::parse_from_reader(&mut reader, args.print)
                    }
                    Err(e) => Err(format!("Error loading input file: {}", e).into()),
                };
                match type_ {
                    &"consum" => imported = res,
                    &"oze" => exported = res,
                    _ => (),
                };
            }
            if let Err(e) = imported {
                Err(e)
            } else if let Err(e) = exported {
                Err(e)
            } else {
                Ok((imported.unwrap(), exported.unwrap()))
            }
        }
        None => {
            //get data from Tauron
            let start = args
                .start
                .unwrap_or(Local::today().naive_local().pred().pred())
                .format("%d.%m.%Y")
                .to_string();
            let end = args
                .end
                .unwrap_or(Local::today().naive_local())
                .format("%d.%m.%Y")
                .to_string();
            match config_read_tauron(conf.clone(), start, end) {
                Ok(scraper) => match scraper.get_json_data().await {
                    Ok((tauron_imported, tauron_exported)) => {
                        //save data to output file when needed
                        if let Some(ref outfile) = args.output {
                            for type_ in &["consum", "oze"] {
                                let outfile = change_file_name(
                                    outfile,
                                    &format!(
                                        "{}_{}",
                                        outfile
                                            .file_stem()
                                            .unwrap()
                                            .to_os_string()
                                            .into_string()
                                            .unwrap(),
                                        type_
                                    ),
                                );
                                info!(
                                    "💾 Saving `{}` JSON data to file: <b><blue>{:?}</>",
                                    type_, &outfile
                                );
                                let events_to_save = match type_ {
                                    &"consum" => Some(&tauron_imported),
                                    &"oze" => Some(&tauron_exported),
                                    _ => None,
                                };
                                if let Some(ev) = events_to_save {
                                    if let Err(e) = std::fs::write(outfile, &ev) {
                                        error!("Unable to write file: {}", e);
                                    }
                                }
                            }
                        }
                        let imported = parser::parse_from_string(tauron_imported, args.print);
                        let exported = parser::parse_from_string(tauron_exported, args.print);
                        if let Err(e) = imported {
                            Err(e)
                        } else if let Err(e) = exported {
                            Err(e)
                        } else {
                            Ok((imported.unwrap(), exported.unwrap()))
                        }
                    }
                    Err(e) => Err(format!("Error obtaining <i>tauron</> data: {}", e).into()),
                },
                Err(e) => Err(format!("Error loading <i>tauron</> config: {}", e).into()),
            }
        }
    };

    //check parsing status and save to db (and output file when configured)
    match entries {
        Ok((imported, exported)) => {
            if let Ok(mut db) = config_read_postgres(conf) {
                tokio::task::spawn_blocking(move || {
                    info!("JSON data parsed correctly");
                    info!("Entries count: <yellow>{}</> for grid import, <yellow>{}</> for grid export", imported.len(), exported.len());
                    info!("🛢️ Trying to store it in the database...");
                    db.insert_data(imported, exported);
                })
                .await
                .expect("Task panicked");
            }
        }
        Err(e) => {
            error!("{}", e);
            return Ok(());
        }
    }

    Ok(())
}
