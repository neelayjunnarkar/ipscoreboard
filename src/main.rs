use warp::Filter;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use rand::Rng;
use std::time::SystemTime;

const LAST_N: usize = 5;
const TOP_N:  usize = 10;
const TOP_N_LAST_MINUTES: usize = 10;
const LAST_N_MINUTES: u64 = 10;
const SYNC_TIME_MINUTES: u64 = 10; 

#[derive(Clone)]
struct Database {
    map: Arc<Mutex<HashMap<String, (u64, Vec<SystemTime>)>>>,
    last_five: Arc<Mutex<VecDeque<String>>>,
}

impl Database {
    fn new() -> Database {
        Database {
            map: Arc::new(Mutex::new(HashMap::new())),
            last_five: Arc::new(Mutex::new(VecDeque::with_capacity(LAST_N))),
        }
    }

    fn increment(&self, ip: String) -> Vec<(String, u64, SystemTime)> {
        let mut map = self.map.lock().unwrap();

        let entry = map.entry(ip).or_insert((0, vec![]));
        (*entry).0 += 1;
        (*entry).1.push(SystemTime::now());

        map.clone().into_iter().map(|(ip, (count, times))| (ip, count, times[times.len()-1])).collect()
    }

    fn update_last_five(&self, ip: String) -> Vec<String> {
        let mut last_five = self.last_five.lock().unwrap();

        if last_five.contains(&ip) {
            last_five.retain(|e| ip != *e);
        }
        if last_five.len() >= LAST_N {
            last_five.pop_back();
        }
        last_five.push_front(ip);

        last_five.clone().into_iter().collect()
    }

    fn new_hit(&self, ip: String) -> (Vec<(String, u64, SystemTime)>, Vec<String>) {
        let counts_vec = self.increment(ip.clone());
        let last_five_vec = self.update_last_five(ip);
        (counts_vec, last_five_vec)
    }

    fn write_to_file(&self) {
        let mut map = self.map.lock().unwrap();
        let _last_five = self.last_five.lock().unwrap();

        // Write to disk.
        
        // Retain only relevant data in memory.
        let curr_time = SystemTime::now();
        for (count, timestamps) in map.values_mut() {
            timestamps.retain(|timestamp| 
                match curr_time.duration_since(*timestamp) {
                    Ok(duration) => duration.as_secs() < 60*LAST_N_MINUTES,
                    Err(_) => false, // If 'earlier' timestamp is after current time, remove it to
                                     // prevent effective memory leak.
                }
            );
            *count = timestamps.len() as u64;
        }
        map.retain(|_, (count, _)| *count != 0); // Keep ips which have hit the website in
                                                 // the last N minutes.
    }
}

fn handle_request(addr: Option<std::net::SocketAddr>, db: Database) -> String {
    let ip = if let Some(addr) = addr {
        addr.ip().to_string()
    } else {
        "0.0.0.0".to_string()
    };

    let (mut counts, last_five) = db.new_hit(ip);

    counts.sort_unstable_by_key(|k| k.1);

    let top_10: Vec<String> = counts.iter().rev().take(TOP_N)
        .map(|(ip, count, _)| ip.to_owned() + ": " +  &count.to_string()).collect();

    let curr_time = SystemTime::now();
    let last_10min_top_10: Vec<String> = counts.iter().rev()
        .filter(|(_, _, timestamp)| true ||
            match curr_time.duration_since(*timestamp) {
                Ok(duration) => duration.as_secs() < 60*LAST_N_MINUTES,
                Err(_) => true, // If 'earlier' timestamp is after current time, accept it.
            }
        )
        .take(TOP_N_LAST_MINUTES)
        .map(|(ip, count, _)| ip.to_owned() + ": " +  &count.to_string()).collect();
    
    let display = vec![
        "Last 5".to_owned(),
        "------".to_owned(),
        last_five.join("\n"),
        "".to_owned(),
        "Top 10".to_owned(),
        "------".to_owned(),
        top_10.join("\n"),
        "".to_owned(),
        "Top 10 in last 10 min".to_owned(),
        "---------------------".to_owned(),
        last_10min_top_10.join("\n")
    ];
    display.join("\n")
}

#[tokio::main]
async fn main() {
    let db = Database::new();

    // Increase size of database for testing
    let num_fake_ips = 1000;
    let distr = rand::distributions::Uniform::<u32>::new(0, 50);
    let mut rng = rand::thread_rng();
    for n in 1..num_fake_ips {
        for _ in 1..rng.sample(distr) {
            db.new_hit(n.to_string());
        }
    }
    println!("Done generating fake database");

    let sync_db = db.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(SYNC_TIME_MINUTES * 60)).await;
            sync_db.write_to_file();
        }
    });

    let routes = warp::addr::remote().map(move |addr| handle_request(addr, db.clone()));
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
