use wordle_solver::core::hard_mode::satisfies_hard_mode;
use wordle_solver::core::solver::auto_solve;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn assert_valid_auto_solve(
    history: &[(Word, wordle_solver::core::pattern::Pattern)],
    target: &str,
) {
    assert!(
        history.last().map(|(_, p)| p.is_win()).unwrap_or(false),
        "did not win solving {target}"
    );
    assert!(history.len() <= 6);
    for i in 0..history.len() {
        let prior: Vec<_> = history[..i].to_vec();
        assert!(
            satisfies_hard_mode(history[i].0, &prior),
            "guess {} turn {} violates hard mode for {target}",
            history[i].0,
            i + 1
        );
    }
}

#[test]
fn auto_solves_sample_words() {
    let lists = WordLists::load();
    for target in [
        "crane", "slate", "eerie", "brood", "hello", "bound", "wound",
    ] {
        let word = Word::from_str(target).unwrap();
        let history = auto_solve(&lists, word).expect("failed to solve {target}");
        assert_valid_auto_solve(&history, target);
    }
}

#[test]
#[ignore]
fn auto_solves_all_answers_within_six_guesses() {
    let lists = WordLists::load();
    let mut failures = Vec::new();
    let mut total_guesses = 0usize;
    let mut worst = 0usize;

    for &target in &lists.answers {
        match auto_solve(&lists, target) {
            Some(history) => {
                assert_valid_auto_solve(&history, target.as_str());
                let n = history.len();
                total_guesses += n;
                worst = worst.max(n);
            }
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
    assert!(
        worst <= 6,
        "worst-case word required {worst} guesses (target <= 6)"
    );
    assert!(
        avg <= 3.56,
        "average guesses too high: {avg:.3} (target <= 3.56)"
    );
}

#[test]
#[ignore]
fn quality_benchmark_stats() {
    let lists = WordLists::load();
    let mut total_guesses = 0usize;
    let mut distribution = [0usize; 6];
    let mut hardest: Vec<(Word, usize)> = Vec::new();

    for &target in &lists.answers {
        let history = auto_solve(&lists, target).expect("solver failed");
        let n = history.len();
        total_guesses += n;
        distribution[n - 1] += 1;
        hardest.push((target, n));
    }

    hardest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let avg = total_guesses as f64 / lists.answers.len() as f64;
    eprintln!("opening guess: {}", lists.opening_guess());
    eprintln!("average guesses: {avg:.4}");
    eprintln!("distribution: {:?}", distribution);
    eprintln!(
        "hardest: {:?}",
        hardest
            .iter()
            .take(10)
            .map(|(w, n)| format!("{w}:{n}"))
            .collect::<Vec<_>>()
    );

    assert!(avg <= 3.56);
    assert!(hardest[0].1 <= 6);
}

#[test]
fn prefers_remaining_answer_on_entropy_tie() {
    use std::collections::HashSet;
    use wordle_solver::core::solver::{compare_one_ply, score_one_ply};

    let lists = WordLists::load();
    let crane = Word::from_str("crate").unwrap();
    let grate = Word::from_str("grate").unwrap();
    let slate = Word::from_str("slate").unwrap();
    let remaining = [crane, grate];
    let set: HashSet<Word> = remaining.iter().copied().collect();

    let from_answers = score_one_ply(&lists, crane, &remaining, &set);
    let probe = score_one_ply(&lists, slate, &remaining, &set);

    if (from_answers.one_ply_entropy - probe.one_ply_entropy).abs() < 1e-9 {
        assert!(compare_one_ply(from_answers, probe) == std::cmp::Ordering::Greater);
    }
}
