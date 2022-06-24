use rand::Rng;
use sqlx::sqlite::SqlitePool;
use sqlx::types::chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use warp::Filter;

const LAST_N: usize = 5;
const TOP_N: usize = 10;
const TOP_N_LAST_MINUTES: usize = 10;
const LAST_N_MINUTES: u64 = 1;
const SYNC_TIME_MINUTES: u64 = 1;

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

    fn increment(&self, ip: String) -> HashMap<String, (u64, Vec<SystemTime>)> {
        let mut map = self.map.lock().unwrap();

        let entry = map.entry(ip).or_insert((0, vec![]));
        entry.0 += 1;
        entry.1.push(SystemTime::now());

        map.clone()
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

    fn new_hit(&self, ip: String) -> (HashMap<String, (u64, Vec<SystemTime>)>, Vec<String>) {
        let counts_vec = self.increment(ip.clone());
        let last_five_vec = self.update_last_five(ip);
        (counts_vec, last_five_vec)
    }

    fn purge_db(&self) -> (SystemTime, HashMap<String, (u64, Vec<SystemTime>)>) {
        let mut map = self.map.lock().unwrap();
        let _last_five = self.last_five.lock().unwrap();

        let db_snapshot = map.clone();

        // Retain only relevant data in memory.
        let curr_time = SystemTime::now();
        for (_, timestamps) in map.values_mut() {
            timestamps.retain(|timestamp| match curr_time.duration_since(*timestamp) {
                Ok(duration) => duration.as_secs() < 60 * LAST_N_MINUTES,
                Err(_) => false, // If 'earlier' timestamp is after current time, remove it to
                                 // prevent effective memory leak.
            });
        }
        // Keep ips which have hit the website in the last N minutes or are in the top 10 all time.
        let mut counts = map
            .values()
            .map(|(count, _)| *count)
            .collect::<Vec<u64>>();
        counts.sort_unstable();
        let top_10_thresh = counts.iter().rev().nth(TOP_N - 1).unwrap_or(&0);
        map.retain(|_, (count, timestamps)| timestamps.len() > 0 || (*count >= *top_10_thresh));

        (curr_time, db_snapshot)
    }
}

fn count_recent_hits(timestamps: &Vec<SystemTime>, curr_time: &SystemTime) -> u64 {
    timestamps
        .iter()
        .filter(|timestamp| {
            curr_time
                .duration_since(**timestamp)
                .map(|duration| duration.as_secs() < 60 * LAST_N_MINUTES)
                .unwrap_or(true)
        })
        .count() as u64
}

fn handle_request(addr: Option<std::net::SocketAddr>, db: Database) -> String {
    let ip = if let Some(addr) = addr {
        addr.ip().to_string()
    } else {
        "0.0.0.0".to_string()
    };

    let (map, last_five) = db.new_hit(ip);

    let curr_time = SystemTime::now();
    let mut counts: Vec<(String, u64, u64)> = map
        .into_iter()
        .map(|(ip, (count, timestamps))| (ip, count, count_recent_hits(&timestamps, &curr_time)))
        .collect();

    counts.sort_unstable_by_key(|k| k.1); // Sort by all time counts.
    let top_10: Vec<String> = counts
        .iter()
        .rev()
        .take(TOP_N)
        .map(|(ip, count, _)| ip.to_owned() + ": " + &count.to_string())
        .collect();

    counts.sort_unstable_by_key(|k| k.2); // Sort by recent counts.
    let last_10min_top_10: Vec<String> = counts
        .iter()
        .rev()
        .filter(|(_, _, recent_count)| *recent_count > 0)
        .take(TOP_N_LAST_MINUTES)
        .map(|(ip, _, recent_counts)| ip.to_owned() + ": " + &recent_counts.to_string())
        .collect();

    let display = vec![
        format!("Last {}", LAST_N),
        "------".to_owned(),
        last_five.join("\n"),
        "".to_owned(),
        format!("Top {}", TOP_N),
        "------".to_owned(),
        top_10.join("\n"),
        "".to_owned(),
        format!("Top {} in last {} min", TOP_N_LAST_MINUTES, LAST_N_MINUTES),
        "---------------------".to_owned(),
        last_10min_top_10.join("\n"),
    ];
    display.join("\n")
}

async fn sync_database(db: Database) {
    let pool = SqlitePool::connect("sqlite:db.sqlite3")
        .await
        .expect("Failed to start pool");

    let mut last_sync_time = None;
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(SYNC_TIME_MINUTES * 60)).await;

        // Purge in-memory database
        let (curr_sync_time, mut write_db) = db.purge_db();

        // Don't write entries that have previously been synced.
        if let Some(last_sync_time) = last_sync_time {
            for (count, timestamps) in write_db.values_mut() {
                timestamps.retain(|timestamp| timestamp > &last_sync_time);
                *count = timestamps.len() as u64;
            }
            write_db.retain(|_, (count, _)| *count != 0); // Keep ips which have hit the website in
        }

        // Write entries to database.
        let mut trans = pool
            .begin()
            .await
            .expect("Failed to begin transaction with database");
        let write_entries = write_db
            .into_iter()
            .map(|(ip, (_, timestamps))| {
                timestamps
                    .into_iter()
                    .map(move |timestamp| (ip.clone(), DateTime::<Utc>::from(timestamp)))
            })
            .flatten();
        for (ip, timestamp) in write_entries {
            sqlx::query!(
                "INSERT INTO hits (timestamp, ip) VALUES (?1, ?2)",
                timestamp,
                ip
            )
            .execute(&mut trans)
            .await
            .expect("Failed to insert (timestamp, ip)");
        }
        trans
            .commit()
            .await
            .expect("Failed to commit transaction to file");

        last_sync_time = Some(curr_sync_time);
        println!("Synced database");
    }
}

#[tokio::main]
async fn main() {
    let db = Database::new();

    // Increase size of database for testing
    let num_fake_ips = 1000;
    let distr = rand::distributions::Uniform::<u32>::new(0, 50);
    let mut rng = rand::thread_rng();
    for n in 1..num_fake_ips {
        let num_hits = rng.sample(distr);
        for _ in 0..(num_hits + 1) {
            db.new_hit(n.to_string());
        }
    }
    println!("Done generating fake database");

    let sync_db = db.clone();
    tokio::spawn(async move {
        sync_database(sync_db).await;
    });

    let routes = warp::addr::remote().map(move |addr| handle_request(addr, db.clone()));
    warp::serve(routes).run(([127, 0, 0, 1], 3002)).await;
}
