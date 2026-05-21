use wordle_solver::core::solver::auto_solve;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

#[test]
fn auto_solves_sample_words() {
    let lists = WordLists::load();
    for target in ["crane", "slate", "eerie", "brood", "hello"] {
        let word = Word::from_str(target).unwrap();
        let result = auto_solve(&lists, word);
        assert!(result.is_some(), "failed to solve {target}");
        assert!(result.unwrap().len() <= 6);
    }
}

#[test]
#[ignore]
fn auto_solves_all_answers_within_six_guesses() {
    let lists = WordLists::load();
    let mut failures = Vec::new();
    let mut total_guesses = 0usize;

    for &target in &lists.answers {
        match auto_solve(&lists, target) {
            Some(history) => total_guesses += history.len(),
            None => failures.push(target),
        }
    }

    if !failures.is_empty() {
        let sample: Vec<_> = failures.iter().take(10).map(|w| w.to_string()).collect();
        panic!(
            "failed to solve {} words, e.g. {:?}. avg guesses for successes: {:.3}",
            failures.len(),
            sample,
            total_guesses as f64 / (lists.answers.len() - failures.len()) as f64
        );
    }

    let avg = total_guesses as f64 / lists.answers.len() as f64;
    assert!(avg <= 3.8, "average guesses too high: {avg:.3}");
}
