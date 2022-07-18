/*
 * This is a very trimmed down version of cowsay that keeps
 * cow files in memory and selects a random one each time.
 */

use rand;
use std::fs;

#[derive(Clone)]
pub struct Cowsay {
    cows: Vec<String>,
}

impl Cowsay {
    pub fn new() -> Cowsay {
        Cowsay { cows: vec![] }
    }

    pub fn load_cows(&mut self, cows_path: &str) {
        for entry in fs::read_dir(cows_path).expect("Failed to read cows directory.") {
            if let Ok(entry) = entry {
                let file_path = entry.path();
                if file_path.is_file() && file_path.extension().unwrap() == "txt" {
                    let cow = fs::read_to_string(file_path).expect("Failed to read cow.");
                    self.cows.push(cow);
                }
            } else {
                println!("Failed to read an entry in the cows directory.");
            }
        }
    }

    pub fn say_random_cow(&self, lines: Vec<String>) -> String {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        if let Some(cow) = self.cows.choose(&mut rng) {
            let joined = vec![speech_bubble(lines), cow.to_string()];
            joined.join("\n")
        } else {
            speech_bubble(lines)
        }
    }
}

fn speech_bubble(mut lines: Vec<String>) -> String {
    let line_width = lines.iter().map(|line| line.len()).max().unwrap();
    let num_lines = lines.len();
    for line in &mut lines[1..num_lines - 1] {
        *line = format!("| {:<1$} |", line, line_width);
    }
    lines[0] = format!("/ {:<1$} \\", lines[0], line_width);
    lines[num_lines - 1] = format!("\\ {:<1$} /", lines[num_lines - 1], line_width);
    lines.insert(0, format!(" {:_<1$}", "_", line_width + 2));
    lines.push(format!(" {:-<1$}", "-", line_width + 2));
    lines.join("\n")
}
