use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone)]
pub struct Database {
    map: Arc<Mutex<HashMap<String, (u64, Vec<SystemTime>)>>>,
    last_five: Arc<Mutex<VecDeque<String>>>,
    last_n: usize,
    top_n: usize,
    last_n_minutes: u64,
}

impl Database {
    pub fn new(last_n: usize, top_n: usize, last_n_minutes: u64) -> Database {
        Database {
            map: Arc::new(Mutex::new(HashMap::new())),
            last_five: Arc::new(Mutex::new(VecDeque::with_capacity(last_n))),
            last_n: last_n,
            top_n: top_n,
            last_n_minutes: last_n_minutes,
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
        if last_five.len() >= self.last_n {
            last_five.pop_back();
        }
        last_five.push_front(ip);

        last_five.clone().into_iter().collect()
    }

    pub fn new_hit(&self, ip: String) -> (HashMap<String, (u64, Vec<SystemTime>)>, Vec<String>) {
        let counts_vec = self.increment(ip.clone());
        let last_five_vec = self.update_last_five(ip);
        (counts_vec, last_five_vec)
    }

    pub fn purge_db(&self) -> (SystemTime, HashMap<String, (u64, Vec<SystemTime>)>) {
        let mut map = self.map.lock().unwrap();
        let _last_five = self.last_five.lock().unwrap();

        let db_snapshot = map.clone();

        // Retain only relevant data in memory.
        let curr_time = SystemTime::now();
        for (_, timestamps) in map.values_mut() {
            timestamps.retain(|timestamp| match curr_time.duration_since(*timestamp) {
                Ok(duration) => duration.as_secs() < 60 * self.last_n_minutes,
                Err(_) => false, // If 'earlier' timestamp is after current time, remove it to
                                 // prevent effective memory leak.
            });
        }
        // Keep ips which have hit the website in the last N minutes or are in the top 10 all time.
        let mut counts = map.values().map(|(count, _)| *count).collect::<Vec<u64>>();
        counts.sort_unstable();
        let top_10_thresh = counts.iter().rev().nth(self.top_n - 1).unwrap_or(&0);
        map.retain(|_, (count, timestamps)| timestamps.len() > 0 || (*count >= *top_10_thresh));

        (curr_time, db_snapshot)
    }
}
