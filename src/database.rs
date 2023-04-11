use openssl::ssl::SslConnector;
use openssl::ssl::SslMethod;
use openssl::ssl::SslVerifyMode;
use postgres_openssl::MakeTlsConnector;
use simplelog::*;
use std::time::Instant;

use crate::parser::Entry;

pub struct Database {
    pub name: String,
    pub host: String,
    pub dbname: String,
    pub username: String,
    pub password: String,
}

impl Database {
    pub fn insert_data(&mut self, entries: Vec<Entry>) {
        let mut builder =
            SslConnector::builder(SslMethod::tls()).expect("SslConnector::builder error");
        builder.set_verify(SslVerifyMode::NONE); //allow self-signed certificates
        let connector = MakeTlsConnector::new(builder.build());

        let connectionstring = format!(
            "postgres://{}:{}@{}/{}?sslmode=require&application_name=elicznik",
            self.username, self.password, self.host, self.dbname
        )
        .to_string()
        .clone();

        info!("{}: Connecting to: <u>{}</>", self.name, connectionstring);
        match postgres::Client::connect(&connectionstring, connector) {
            Ok(mut client) => {
                info!("{}: Connected successfully", self.name);
                info!("{}: Storing entries...", self.name);
                let started = Instant::now();

                let mut inserted = 0;
                let mut updated = 0;
                for e in &entries {
                    let result = client.query("select * from tauron_add_entry($1::date, $2::smallint, $3::boolean, true, $4::float)",
                            &[&e.Date, &(e.Hour as i16), &e.Extra, &e.EC]);
                    match result {
                        Ok(rows) => {
                            for row in &rows {
                                updated =
                                    updated + row.try_get::<_, i32>("updated").unwrap_or_default();
                                inserted = inserted
                                    + row.try_get::<_, i32>("inserted").unwrap_or_default();
                            }
                        }
                        Err(e) => {
                            error!("{}: Problem executing query: {:?}", self.name, e);
                        }
                    }
                }
                info!(
                    "{}: Grid entries: <yellow>{}</> processed <black>=></> <yellow>{}</> inserted, <yellow>{}</> updated",
                    self.name, entries.len(), inserted, updated
                );

                let elapsed = started.elapsed();
                let ms = (elapsed.as_secs() * 1_000) + (elapsed.subsec_nanos() / 1_000_000) as u64;
                info!("{}: ⌛ total SQL time: <magenta>{}</> ms", self.name, ms);
            }
            Err(e) => {
                error!("{}: PostgreSQL connection error: {:?}", self.name, e);
            }
        }
    }
}
