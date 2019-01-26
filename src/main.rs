#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate serde_json;

extern crate curl;
use curl::easy::Easy;

extern crate rusqlite;
use rusqlite::types::ToSql;
use rusqlite::{Connection, NO_PARAMS};

use std::process::Command;

extern crate iron;
use std::sync::{Arc,Mutex};
use iron::prelude::*;
use iron::status;

#[macro_use] extern crate hyper;

header! { (XRealIP, "X-Real-IP") => [String] }

#[derive(Deserialize)]
struct Geolookup {
    ip: String,
    hostname: String,
    city: String,
    region: String,
    country: String,
    loc: String,
    postal: String,
    org: String,
}

fn main() {
    let conn = Connection::open("db.sqlite3").unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hits (
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            ip TEXT NOT NULL
            )",
            NO_PARAMS
    ).expect("failed on create table");;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS geolookups (
            ip TEXT NOT NULL,
            hostname TEXT NOT NULL,
            city TEXT NOT NULL,
            region TEXT NOT NULL,
            country TEXT NOT NULL,
            loc TEXT NOT NULL,
            postal TEXT NOT NULL,
            org TEXT NOT NULL
            )",
            NO_PARAMS
    ).expect("failed on create geolookups table");

    let conns = Arc::new(Mutex::new(conn));

    Iron::new(move |req: &mut Request| {
        if let Some(ip_addr_s) = req.headers.get::<XRealIP>() {
            if let Ok(conn) = conns.lock() {
                let ip_addr_v = (&ip_addr_s).split(',').collect::<Vec<&str>>();
                let ip_addr = ip_addr_v[ip_addr_v.len()-1].trim().to_string();

                conn.execute(
                    "INSERT INTO hits (ip)
                            VALUES (?1)",
                            &[&ip_addr as &ToSql],
                            ).unwrap_or(0);

                /* Check if ip already seen and otherwise geolookup and add to table */
                if let Ok(mut stmt) = conn.prepare("SELECT 1 FROM geolookups WHERE ip=?1 LIMIT 1") {
                    if let Ok(exists) = stmt.exists(&[&ip_addr as &ToSql]) {
                        if !exists {
                            /* geolookup ip */
                            let mut easy = Easy::new();
                            let mut data = Vec::new();
                            if easy.url(&("http://ipinfo.io/".to_owned() + &ip_addr)).is_ok() {
                                {
                                    let mut transfer = easy.transfer();
                                    transfer.write_function(|new_data| {
                                        println!("{}",String::from_utf8(new_data.to_vec()).expect("fail on string from utf8"));
                                        data.extend_from_slice(new_data);
                                        Ok(new_data.len())
                                    }).expect("fail on easy write fn");
                                    transfer.perform().expect("fail on easy perform");
                                }
                                if let Ok(v) = serde_json::from_slice(&data) as serde_json::Result<Geolookup> {
                                    conn.execute(
                                        "INSERT INTO geolookups (ip, hostname, city, region, country, loc, postal, org)
                                        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                                        &[&v.ip as &ToSql, &v.hostname as &ToSql, &v.city as &ToSql, &v.region as &ToSql, &v.country as &ToSql, &v.loc as &ToSql, &v.postal as &ToSql, &v.org as &ToSql],
                                        ).expect("failed to insert into geolookups");
                                }
                            }
                        }
                    }
                }

                /* Create response */
                let mut response: String = String::new();

                /* Add Last 5 */
                response.push_str("Last 5\\n======\\n");
                if let Ok(mut stmt) = conn.prepare("SELECT ip FROM hits GROUP BY ip HAVING timestamp=MAX(timestamp) ORDER BY timestamp DESC LIMIT 5") {
                    if let Ok(mut rows) = stmt.query(NO_PARAMS) {
                        while let Some(row) = rows.next() {
                            if let Ok(row) = row {
                                let ip: String = row.get(0);
                                response.push_str(ip.as_str());
                                response.push('\n');
                            }
                        }
                    }
                }

                /* Add Top 10*/
                response.push_str("\\nTop 10\n======\\n");
                if let Ok(mut stmt) = conn.prepare("SELECT ip, COUNT(*) AS count FROM hits GROUP BY ip ORDER BY count DESC LIMIT 10") {
                    if let Ok(mut rows) = stmt.query(NO_PARAMS) {
                        while let Some(row) = rows.next() {
                            if let Ok(row) = row {
                                let ip: String = row.get(0);
                                let count: i64 = row.get(1);
                                response.push_str(ip.as_str());
                                response.push_str(": ");
                                response.push_str(count.to_string().as_str());
                                response.push('\n');
                            }
                        }
                    }
                }

                /* Add Top 10 in last 10 min */
                response.push_str("\\nTop 10 in last 10 min\\n=====================\\n");
                if let Ok(mut stmt) = conn.prepare("SELECT ip, COUNT(*) AS count FROM hits WHERE timestamp>datetime('now', '-10 minute') GROUP BY ip ORDER BY count DESC LIMIT 10") {
                    if let Ok(mut rows) = stmt.query(NO_PARAMS) {
                        while let Some(row) = rows.next() {
                            if let Ok(row) = row {
                                let ip: String = row.get(0);
                                let count: i64 = row.get(1);
                                response.push_str(ip.as_str());
                                response.push_str(": ");
                                response.push_str(count.to_string().as_str());
                                response.push('\n');
                            }
                        }
                    }
                }

                response = String::from_utf8(Command::new("./cowsayer.sh")
                                             .args(vec![response.clone()])
                                             .output()
                                             .expect("failed to run cowsayer")
                                             .stdout).unwrap_or(response);

                return Ok(Response::with((status::Ok, response)));
            }
        }
        Ok(Response::with((status::Ok, "something weird about how you're accessing this site\n"))) 
    }).http("localhost:3001").unwrap();
}
