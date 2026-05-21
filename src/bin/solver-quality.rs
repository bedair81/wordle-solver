use std::collections::HashMap;

use wordle_solver::core::solver::auto_solve;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn main() {
    let lists = WordLists::load();
    let opening = lists.opening_guess();
    println!("Computed opening guess: {opening}");
    println!();

    let mut total_guesses = 0usize;
    let mut failures = Vec::new();
    let mut distribution = [0usize; 6];
    let mut by_guesses: HashMap<usize, Vec<Word>> = HashMap::new();

    for &target in &lists.answers {
        match auto_solve(&lists, target) {
            Some(history) => {
                let n = history.len();
                total_guesses += n;
                if n >= 1 && n <= 6 {
                    distribution[n - 1] += 1;
                }
                by_guesses.entry(n).or_default().push(target);
            }
            None => failures.push(target),
        }
    }

    let solved = lists.answers.len() - failures.len();
    let avg = total_guesses as f64 / solved as f64;
    let worst = by_guesses.keys().copied().max().unwrap_or(0);

    println!("Solved: {solved} / {}", lists.answers.len());
    println!("Failed: {}", failures.len());
    println!("Average guesses: {avg:.4}");
    println!("Worst case: {worst} guesses");
    println!();
    println!("Distribution:");
    for (i, &count) in distribution.iter().enumerate() {
        println!("  {} guesses: {count}", i + 1);
    }

    if let Some(hard) = by_guesses.get(&worst) {
        println!();
        println!("Hardest words ({worst} guesses):");
        let mut hard = hard.clone();
        hard.sort();
        for word in hard.iter().take(10) {
            println!("  {word}");
        }
    }

    if !failures.is_empty() {
        println!();
        println!("Failures (first 10):");
        for word in failures.iter().take(10) {
            println!("  {word}");
        }
        std::process::exit(1);
    }
}
