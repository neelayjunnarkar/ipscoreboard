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

fn main() {
    let conn = Connection::open("db.sqlite3").unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hits (
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            ip TEXT NOT NULL
            )",
            NO_PARAMS
    ).expect("failed on create table");;

    let conn = Arc::new(Mutex::new(conn));

    Iron::new(move |req: &mut Request| {
        if let Some(ip_addr_s) = req.headers.get::<XRealIP>() {
            if let Ok(conn) = conn.lock() {
                let ip_addr_v = (&ip_addr_s).split(',').collect::<Vec<&str>>();
                let ip_addr = ip_addr_v[ip_addr_v.len()-1].trim().to_string();

                conn.execute(
                    "INSERT INTO hits (ip)
                            VALUES (?1)",
                            &[&ip_addr as &ToSql],
                            ).expect("failed on insert");

                let mut response: String = String::new();
                response.push_str("Last 5\\n======\\n");

                let mut stmt = conn.prepare("SELECT ip FROM hits GROUP BY ip HAVING timestamp=MAX(timestamp) ORDER BY timestamp DESC LIMIT 5").expect("failed on prepare");
                let mut rows = stmt.query(NO_PARAMS).expect("failed on query");

                while let Some(row) = rows.next() {
                    let row = row.unwrap();
                    let ip: String = row.get(0);
                    response.push_str(ip.as_str());
                    response.push('\n');
                }

                response.push_str("\\nTop 10\n======\\n");

                let mut stmt = conn.prepare("SELECT ip, COUNT(*) AS count FROM hits GROUP BY ip ORDER BY count DESC LIMIT 10").expect("failed on other prepare");
                let mut rows = stmt.query(NO_PARAMS).expect("failed on query2"); 

                while let Some(row) = rows.next() {
                    let row = row.unwrap();
                    let ip: String = row.get(0);
                    let count: i64 = row.get(1);
                    response.push_str(ip.as_str());
                    response.push_str(": ");
                    response.push_str(count.to_string().as_str());
                    response.push('\n');
                }

                let mut stmt = conn.prepare("SELECT ip, COUNT(*) AS count FROM hits WHERE timestamp>datetime('now', '-10 minute') GROUP BY ip ORDER BY count DESC LIMIT 10").expect("fail on rate prep");
                let mut rows = stmt.query(NO_PARAMS).expect("failed on rate query");

                response.push_str("\\nHits in last 10 min\\n===================\\n");

                while let Some(row) = rows.next() {
                    let row = row.unwrap();
                    let ip: String = row.get(0);
                    let count: i64 = row.get(1);
                    response.push_str(ip.as_str());
                    response.push_str(": ");
                    response.push_str(count.to_string().as_str());
                    response.push('\n');
                }

                response = String::from_utf8(Command::new("./cowsayer.sh")
                                             .args(vec![response])
                                             .output()
                                             .expect("failed to run cowsayer")
                                             .stdout).expect("failed to cvt output of cowsayer to string");

                return Ok(Response::with((status::Ok, response)));
            }
        }
        Ok(Response::with((status::Ok, "something weird about how you're accessing this site\n"))) 
    }).http("localhost:3001").unwrap();
}
