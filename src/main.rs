extern crate rusqlite;
use rusqlite::types::ToSql;
use rusqlite::{Connection, NO_PARAMS};

use std::process::Command;

extern crate iron;
use std::collections::{HashMap, VecDeque};
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
    /*
       conn.execute(
       "CREATE TABLE IF NOT EXISTS counts (
       ip TEXT NOT NULL,
       count INTEGER NOT NULL
       )",
       NO_PARAMS
       ).unwrap();
       */
    let conn = Arc::new(Mutex::new(conn));

    let counts: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    Iron::new(move |req: &mut Request| {
        if let Ok(mut counts) = counts.lock() {
            if let Some(ip_addr_s) = req.headers.get::<XRealIP>() {
                if let Ok(conn) = conn.lock() {
                    let ip_addr_v = (&ip_addr_s).split(',').collect::<Vec<&str>>();
                    let ip_addr = ip_addr_v[ip_addr_v.len()-1].trim().to_string();

                    conn.execute(
                        "INSERT INTO hits (ip)
                            VALUES (?1)",
                            &[&ip_addr as &ToSql],
                            ).expect("failed on insert");
                    /*
                       conn.execute(
                       "INSERT INTO counts (ip, count)
                       VALUES (?1, ?2)
                       ",
                       &[&ip_addr as &ToSql],
                       ).unwrap();
                       */

                    let mut response: String = String::new();
                    response.push_str("Last 5\\n======\\n");

                    let mut stmt = conn.prepare("SELECT ip FROM hits GROUP BY ip HAVING timestamp=MAX(timestamp) ORDER BY timestamp DESC LIMIT 5").expect("failed on prepare");
                    let mut rows = stmt.query(NO_PARAMS).expect("failed on query");

                    while let Some(row) = rows.next() {
                        let row = row.unwrap();
                        let ip: String = row.get(0);
                        response.push_str(ip.as_str());
                        response.push_str("\n");
                    }

                    {
                        let count = (*counts).entry(ip_addr).or_insert(0);
                        *count += 1;
                    }

                    let mut sorted = (*counts).iter().collect::<Vec<(&String, &u64)>>();
                    sorted.sort_by(|a,b| b.1.cmp(a.1));
                    let top5_str = sorted.iter().take(5).fold(String::new(), |acc, x| acc + x.0 + ": " + &x.1.to_string() + "\\n");


                    response.push_str("\\nTop 5\\n=====\\n");
                    response.push_str(&top5_str);

                    response = String::from_utf8(Command::new("./cowsayer.sh")
                                                 .args(vec![response])
                                                 .output()
                                                 .expect("failed to run cowsayer")
                                                 .stdout).expect("failed to cvt output of cowsayer to string");

                    return Ok(Response::with((status::Ok, response)));
                }
            }
        }
        Ok(Response::with((status::Ok, "something weird about how you're accessing this site\n"))) 
    }).http("localhost:3001").unwrap();
}
