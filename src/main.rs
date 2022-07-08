use rand::Rng;
use sqlx::sqlite::SqlitePool;
use sqlx::types::chrono::{DateTime, Utc};
use std::time::SystemTime;
use warp::Filter;

mod database;
use database::Database;

const LAST_N: usize = 5;
const TOP_N: usize = 10;
const TOP_N_LAST_MINUTES: usize = 10;
const LAST_N_MINUTES: u64 = 10;
const SYNC_TIME_MINUTES: u64 = 1;
const DATABASE_PATH: &'static str = "sqlite:db.sqlite3";
const SERVER_PORT: u16 = 3002;

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

fn handle_request(ip: String, db: Database) -> String {
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

async fn sync_database(db: Database, pool: &SqlitePool) {
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

async fn load_database(db: &Database, pool: &SqlitePool) {
    let hits_top_n = sqlx::query!(r#"SELECT ip, COUNT(ip) as "count: u32" FROM hits GROUP BY ip ORDER BY COUNT(ip) DESC LIMIT ?"#, TOP_N as u32)
        .fetch_all(pool)
        .await
        .expect("Failed to read top 10 from database.");

    let last_n_minutes = format!("-{LAST_N_MINUTES}");
    let hits_last_n_minutes = sqlx::query!(r#"SELECT ip, timestamp FROM hits WHERE timestamp >= DATETIME('NOW', ?) ORDER BY timestamp ASC"#, last_n_minutes)
        .fetch(pool)
        .await
        .expect("Failed to read hits in last n minutes.");
}

#[tokio::main]
async fn main() {
    let pool = SqlitePool::connect(DATABASE_PATH)
        .await
        .expect("Failed to start pool");

    let db = Database::new(LAST_N, TOP_N, LAST_N_MINUTES);
    load_database(&db, &pool).await;

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
        sync_database(sync_db, &pool).await;
    });

    let routes = warp::header("X-Real-IP").map(move |ip| handle_request(ip, db.clone()));
    warp::serve(routes).run(([127, 0, 0, 1], SERVER_PORT)).await;
}
