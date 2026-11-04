// Median cost of one spawn-and-join, the most the memo and wave threads could save per thread.
use std::time::Instant;
fn main() {
    let mut samples: Vec<u128> = (0..5000)
        .map(|_| {
            let t = Instant::now();
            std::thread::spawn(|| {}).join().unwrap();
            t.elapsed().as_nanos()
        })
        .collect();
    samples.sort_unstable();
    println!("spawn+join ns: median {} p90 {}", samples[2500], samples[4500]);
}
