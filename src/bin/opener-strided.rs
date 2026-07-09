//! Strided full-solve opener comparison (fast proxy for full opening-benchmark).
use wordle_solver::core::solver::auto_solve_with_options;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn main() {
    let lists = WordLists::load();
    let openers = ["soare", "roate", "raise", "slate", "salet", "crane"];
    let stride: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(23);
    let sample: Vec<_> = lists.answers.iter().step_by(stride).copied().collect();
    println!("sample_size={} stride={stride}", sample.len());
    for name in openers {
        let opener = Word::parse(name).unwrap();
        if !lists.is_valid_guess(opener) {
            println!("{name}: SKIP not in pool");
            continue;
        }
        let mut total = 0usize;
        let mut fails = 0usize;
        let mut worst = 0usize;
        for &t in &sample {
            match auto_solve_with_options(&lists, t, false, opener) {
                Some(h) => {
                    total += h.len();
                    worst = worst.max(h.len());
                }
                None => fails += 1,
            }
        }
        let solved = sample.len() - fails;
        let avg = if solved > 0 {
            total as f64 / solved as f64
        } else {
            0.0
        };
        println!(
            "{name}: avg={avg:.4} worst={worst} fails={fails}/{n}",
            n = sample.len()
        );
    }
}
