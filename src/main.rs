use futures::TryStreamExt;
use sqlx::sqlite::SqlitePool;
use sqlx::types::chrono::{DateTime, Local, TimeZone, Utc};
use std::time::SystemTime;
use warp::Filter;

mod database;
use database::Database;

mod cowsay;
use cowsay::Cowsay;

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

fn handle_request(ip: String, db: Database, cowsay: &Cowsay) -> String {
    let (map, mut last_five) = db.new_hit(ip);

    let curr_time = SystemTime::now();
    let mut counts: Vec<(String, u64, u64)> = map
        .into_iter()
        .map(|(ip, (count, timestamps))| (ip, count, count_recent_hits(&timestamps, &curr_time)))
        .collect();

    counts.sort_unstable_by_key(|k| k.1); // Sort by all time counts.
    let mut top_10: Vec<String> = counts
        .iter()
        .rev()
        .take(TOP_N)
        .map(|(ip, count, _)| ip.to_owned() + ": " + &count.to_string())
        .collect();

    counts.sort_unstable_by_key(|k| k.2); // Sort by recent counts.
    let mut last_10min_top_10: Vec<String> = counts
        .iter()
        .rev()
        .filter(|(_, _, recent_count)| *recent_count > 0)
        .take(TOP_N_LAST_MINUTES)
        .map(|(ip, _, recent_counts)| ip.to_owned() + ": " + &recent_counts.to_string())
        .collect();

    let mut lines = vec![format!("Last {}", LAST_N), "------".to_string()];
    lines.append(&mut last_five);
    lines.push("".to_string());
    lines.push(format!("Top {}", TOP_N));
    lines.push("------".to_string());
    lines.append(&mut top_10);
    lines.push("".to_string());
    lines.push(format!(
        "Top {} in last {} min",
        TOP_N_LAST_MINUTES, LAST_N_MINUTES
    ));
    lines.push("---------------------".to_string());
    lines.append(&mut last_10min_top_10);
    cowsay.say_random_cow(lines)
}

async fn sync_database(db: Database, pool: &SqlitePool) {
    // Set to current time so values read from database are not duplicated.
    let mut last_sync_time = Some(SystemTime::now());
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
    // Read top N from database.
    let hits_top_n = sqlx::query!(r#"SELECT ip, COUNT(ip) AS "count: u32" FROM hits GROUP BY ip ORDER BY COUNT(ip) DESC LIMIT ?"#, TOP_N as u32)
        .fetch_all(pool)
        .await
        .expect("Failed to read top N from database.");
    for record in hits_top_n.into_iter() {
        if let (Some(ip), Some(count)) = (record.ip, record.count) {
            db.set_all_time_hits(ip, count.into());
        } else {
            println!("Failed to read a top ip count");
        }
    }

    // Read hits from last N minutes.
    let last_n_minutes = format!("-{LAST_N_MINUTES} minute");
    let mut hits_last_n_minutes = sqlx::query!(r#"SELECT ip, timestamp FROM hits WHERE timestamp >= DATETIME('NOW', ?) ORDER BY timestamp ASC"#, last_n_minutes)
        .fetch(pool);
    while let Some(hit) = hits_last_n_minutes
        .try_next()
        .await
        .expect("Failed to read next row")
    {
        let ip = hit.ip;
        let timestamp =
            SystemTime::from(Local.from_local_datetime(&hit.timestamp.unwrap()).unwrap());
        db.insert_timestamp(ip.clone(), timestamp);
        if db.get_all_time_hits(&ip) == 0 {
            let ip_all_time_hits = sqlx::query!(
                r#"SELECT COUNT(ip) AS "count: u32" FROM hits WHERE ip == ?"#,
                ip
            )
            .fetch_one(pool)
            .await
            .expect("Failed to read total number of hits for ip")
            .count;
            db.set_all_time_hits(ip, ip_all_time_hits.into());
        }
    }

    // Read last N hits.
    let last_n_hits = sqlx::query!(
        r#"SELECT ip FROM hits ORDER BY timestamp DESC LIMIT ?"#,
        LAST_N as u32
    )
    .fetch_all(pool)
    .await
    .expect("Failed to read last N from database.");
    for record in last_n_hits.into_iter().rev() {
        if let Some(ip) = record.ip {
            db.update_last_five(ip);
        } else {
            println!("Failed to read one of the last N ips.");
        }
    }
}

#[tokio::main]
async fn main() {
    let pool = SqlitePool::connect(DATABASE_PATH)
        .await
        .expect("Failed to start pool");

    let db = Database::new(LAST_N, TOP_N, LAST_N_MINUTES);
    load_database(&db, &pool).await;
    println!("Done loading database");

    let mut cowsay = cowsay::Cowsay::new();
    cowsay.load_cows("cows");

    let sync_db = db.clone();
    tokio::spawn(async move {
        sync_database(sync_db, &pool).await;
    });

    let routes = warp::header("X-Real-IP").map(move |ip| handle_request(ip, db.clone(), &cowsay));
    warp::serve(routes).run(([127, 0, 0, 1], SERVER_PORT)).await;
}
