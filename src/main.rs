extern crate rusqlite;
use rusqlite::Connection;

use std::process::Command;

extern crate iron;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc,Mutex};
use iron::prelude::*;
use iron::status;

#[macro_use] extern crate hyper;

header! { (XRealIP, "X-Real-IP") => [String] }

fn main() {
    let conn = Connection::open_in_memory().unwrap();

    let last5: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::with_capacity(5)));
    let counts: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    Iron::new(move |req: &mut Request| {
        if let Ok(mut last5) = last5.lock() {
            if let Ok(mut counts) = counts.lock() {
                if let Some(ip_addr_s) = req.headers.get::<XRealIP>() {
                    let ip_addr_v = (&ip_addr_s).split(',').collect::<Vec<&str>>();
                    let ip_addr = ip_addr_v[ip_addr_v.len()-1].trim().to_string();

                    if !(*last5).contains(&ip_addr) {
                        if (*last5).len() == 5 {
                            (*last5).pop_back(); 
                        }
                        (*last5).push_front(ip_addr.clone()); 
                    }

                    {
                        let count = (*counts).entry(ip_addr).or_insert(0);
                        *count += 1;
                    }
                    
                    let last5_str = (*last5).iter().fold(String::new(), |acc, s| acc + s + "\\n");
                    let mut sorted = (*counts).iter().collect::<Vec<(&String, &u64)>>();
                    sorted.sort_by(|a,b| b.1.cmp(a.1));
                    let top5_str = sorted.iter().take(5).fold(String::new(), |acc, x| acc + x.0 + ": " + &x.1.to_string() + "\\n");

                    let mut response: String = String::new();
                    response.push_str("Last 5\\n======\\n");
                    response.push_str(&last5_str);
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
