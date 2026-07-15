use std::collections::HashSet;

use wordle_solver::core::hard_mode::satisfies_hard_mode;
use wordle_solver::core::solver::{
    auto_solve, compare_final, compute_suggestion, score_one_ply, score_two_ply,
};
use wordle_solver::core::word::Word;
use wordle_solver::core::words::shared_word_lists;

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
    let lists = shared_word_lists();
    for target in [
        "crane", "slate", "eerie", "brood", "hello", "bound", "wound",
    ] {
        let word = Word::parse(target).unwrap();
        let history = auto_solve(&lists, word).expect("failed to solve {target}");
        assert_valid_auto_solve(&history, target);
    }
}

#[test]
#[ignore]
fn auto_solves_all_answers_within_six_guesses() {
    let lists = shared_word_lists();
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
    let lists = shared_word_lists();
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

const FAST_HARD_CASES: &[&str] = &[
    "boxer", "batty", "billy", "breed", "bunch", "cater", "cheer", "creak", "pound", "wound",
    "haunt", "waste", "bound", "found", "hound", "round", "sound",
];

/// Fast smoke: known hard cases only (~5–6s release, ~1–2 min debug). Always runs in CI.
#[test]
fn auto_solves_hard_cases_smoke() {
    let lists = shared_word_lists();
    for &target in FAST_HARD_CASES {
        let word = Word::parse(target).unwrap();
        let history = auto_solve(&lists, word).unwrap_or_else(|| panic!("failed {target}"));
        assert_valid_auto_solve(&history, target);
    }
}

/// Strided quality sample (~50 words). Ignored by default: ~7s release, ~4 min debug.
/// Run before releases: `cargo test --release auto_solves_strided_sample -- --ignored`
#[test]
#[ignore = "strided quality sample; run in release before releases"]
fn auto_solves_strided_sample() {
    let lists = shared_word_lists();
    let strided: Vec<Word> = lists.answers.iter().step_by(46).copied().collect();
    assert!(
        strided.len() >= 50,
        "expected at least 50 strided targets, got {}",
        strided.len()
    );

    let mut failures = Vec::new();
    let mut total = 0usize;
    let mut worst = 0usize;

    for &word in &strided {
        match auto_solve(&lists, word) {
            Some(history) => {
                assert_valid_auto_solve(&history, word.as_str());
                let n = history.len();
                total += n;
                worst = worst.max(n);
            }
            None => failures.push(word),
        }
    }

    assert!(
        failures.is_empty(),
        "failed strided words: {:?}",
        failures.iter().take(5).collect::<Vec<_>>()
    );
    let avg = total as f64 / strided.len() as f64;
    assert!(worst <= 6, "worst case {worst} in strided sample");
    assert!(
        avg <= 3.61,
        "strided sample average {avg:.3} too high (full benchmark <= 3.56; sample ~3.61)"
    );
}

#[test]
fn compare_final_picks_better_partition_in_ound_cluster() {
    let lists = shared_word_lists();
    let remaining = [
        Word::parse("bound").unwrap(),
        Word::parse("found").unwrap(),
        Word::parse("hound").unwrap(),
        Word::parse("mound").unwrap(),
        Word::parse("pound").unwrap(),
        Word::parse("round").unwrap(),
        Word::parse("sound").unwrap(),
        Word::parse("wound").unwrap(),
    ];
    let remaining_set: HashSet<Word> = remaining.iter().copied().collect();
    let slate_word = Word::parse("slate").unwrap();
    let taint_word = Word::parse("taint").unwrap();
    let slate_score = score_one_ply(&lists, slate_word, &remaining, &remaining_set);
    let taint_score = score_one_ply(&lists, taint_word, &remaining, &remaining_set);
    let slate = score_two_ply(
        &lists,
        slate_score,
        &remaining,
        &remaining_set,
        &[],
        Some(3),
    );
    let taint = score_two_ply(
        &lists,
        taint_score,
        &remaining,
        &remaining_set,
        &[],
        Some(3),
    );
    assert_eq!(
        compare_final(slate, taint, Some(3), 8),
        std::cmp::Ordering::Greater
    );

    let guesses = ["bound", "sound", "taint", "slate"];
    let refined: Vec<_> = guesses
        .iter()
        .map(|s| Word::parse(s).unwrap())
        .map(|word| score_one_ply(&lists, word, &remaining, &remaining_set))
        .map(|score| score_two_ply(&lists, score, &remaining, &remaining_set, &[], Some(3)))
        .collect();
    let best = refined
        .iter()
        .max_by(|a, b| compare_final(**a, **b, Some(3), 8))
        .expect("guesses scored");
    assert_eq!(best.word, Word::parse("slate").unwrap());
    assert_eq!(best.worst_bucket, 7);
}

#[test]
fn compute_suggestion_respects_turns_left_in_endgame() {
    let lists = shared_word_lists();
    let remaining: Vec<Word> = [
        "bound", "found", "hound", "mound", "pound", "round", "sound", "wound",
    ]
    .iter()
    .map(|s| Word::parse(s).unwrap())
    .collect();
    let with_turns = compute_suggestion(&lists, &remaining, &[], Some(3), false).unwrap();
    let open_ended = compute_suggestion(&lists, &remaining, &[], None, false).unwrap();

    let max_bucket = |guess: Word| {
        lists
            .pattern_cache
            .build_buckets_for(guess, &remaining)
            .counts
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    };
    let bound = Word::parse("bound").unwrap();
    let max_with = max_bucket(with_turns.word);
    let max_open = max_bucket(open_ended.word);
    let max_bound = max_bucket(bound);

    // Prior quality: offlist probes like barfs/herms achieve max_bucket ~4; guessing a
    // remaining *ound word leaves max_bucket 7. Turns-aware must not regress to that trap.
    assert!(
        max_with <= 4,
        "turns-aware pick {} max_bucket={max_with} (want <=4; bound trap={max_bound})",
        with_turns.word
    );
    assert!(
        max_with <= max_open,
        "turns-aware {} (max={max_with}) must not lose to open-ended {} (max={max_open})",
        with_turns.word,
        open_ended.word
    );
    assert!(
        !remaining.contains(&with_turns.word),
        "with 8 remaining / 3 turns should probe offlist, got {}",
        with_turns.word
    );
}

#[test]
fn hard_mode_suggestion_compliant_after_slate_miss() {
    let lists = shared_word_lists();
    let history = vec![(
        Word::parse("slate").unwrap(),
        wordle_solver::core::pattern::Pattern::from_str("xxxxx").unwrap(),
    )];
    let remaining = wordle_solver::core::filter::filter_by_history(&lists.answers, &history);
    let s = compute_suggestion(&lists, &remaining, &history, Some(5), false).unwrap();
    assert!(satisfies_hard_mode(s.word, &history));
    assert_eq!(s.word.as_str().len(), 5);
}

#[test]
fn refined_scores_prefer_lower_expected_guesses_via_shipped_api() {
    use wordle_solver::core::solver::{compare_final, score_one_ply, score_two_ply};
    let lists = shared_word_lists();
    let remaining = [
        Word::parse("bound").unwrap(),
        Word::parse("found").unwrap(),
        Word::parse("hound").unwrap(),
        Word::parse("mound").unwrap(),
        Word::parse("pound").unwrap(),
        Word::parse("round").unwrap(),
        Word::parse("sound").unwrap(),
        Word::parse("wound").unwrap(),
    ];
    let remaining_set: HashSet<Word> = remaining.iter().copied().collect();
    let a = score_two_ply(
        &lists,
        score_one_ply(&lists, Word::parse("slate").unwrap(), &remaining, &remaining_set),
        &remaining,
        &remaining_set,
        &[],
        Some(3),
    );
    let b = score_two_ply(
        &lists,
        score_one_ply(&lists, Word::parse("taint").unwrap(), &remaining, &remaining_set),
        &remaining,
        &remaining_set,
        &[],
        Some(3),
    );
    assert!(a.refined && b.refined);
    assert!(a.expected_guesses.is_finite() && b.expected_guesses.is_finite());
    // Partition-tight turns: slate's better worst-bucket must win through shipped compare_final.
    assert_eq!(
        compare_final(a, b, Some(3), remaining.len()),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn second_guess_lookup_path_after_opener_is_consistent() {
    use wordle_solver::core::solver::lookup_second_guess;
    use wordle_solver::core::words::OPENING_GUESS;
    let lists = shared_word_lists();
    let history = vec![(
        OPENING_GUESS,
        wordle_solver::core::pattern::Pattern::from_str("xxxxx").unwrap(),
    )];
    let remaining = wordle_solver::core::filter::filter_by_history(&lists.answers, &history);
    let live = compute_suggestion(&lists, &remaining, &history, Some(5), false).unwrap();
    assert!(satisfies_hard_mode(live.word, &history));
    assert_eq!(live.word.as_str().len(), 5);
    // When the table has an entry, suggestion must match it; when empty, live search still works.
    if let Some(table_word) = lookup_second_guess(&history, OPENING_GUESS) {
        assert_eq!(
            live.word, table_word,
            "compute_suggestion must use precomputed second guess when present"
        );
    }
}

#[test]
fn copilot_session_after_mason_still_suggests_pshaw() {
    use wordle_solver::core::game::GameState;
    use wordle_solver::core::pattern::Pattern;
    use wordle_solver::core::word::Word;
    use wordle_solver::core::words::shared_word_lists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }
    fn p(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    let lists = shared_word_lists();
    assert!(
        lists.is_answer(w("pshaw")),
        "pshaw must be in answers.txt (NYT solution missing from older lists)"
    );

    let mut game = GameState::new(lists);
    game.record_turn(w("slate"), p("YXYXX")).unwrap();
    game.record_turn(w("abyss"), p("YXXYX")).unwrap();
    game.record_turn(w("mason"), p("XYYXX")).unwrap();

    assert!(
        game.remaining_answers().contains(&w("pshaw")),
        "pshaw should remain after slate/abyss/mason feedback"
    );

    let suggestion = game
        .suggest_next()
        .expect("must still suggest after mason is ruled out");
    assert_eq!(suggestion.word, w("pshaw"));
}

#[test]
fn guess_pool_fallback_suggests_when_answers_empty() {
    use wordle_solver::core::filter::filter_by_history;
    use wordle_solver::core::pattern::Pattern;
    use wordle_solver::core::solver::suggest_guess_with_options;
    use wordle_solver::core::word::Word;
    use wordle_solver::core::words::shared_word_lists;
    use wordle_solver::core::words::OPENING_GUESS;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }
    fn p(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    let lists = shared_word_lists();
    // History that leaves no answers.txt word but still matches guess-pool words.
    // Construct by taking a non-answer guess-pool word and synthesizing consistent feedback
    // via a word that is answers-only-empty: use pshaw-shaped history but pretend answers
    // filter returned empty by passing &[].
    let history = vec![
        (w("slate"), p("YXYXX")),
        (w("abyss"), p("YXXYX")),
        (w("mason"), p("XYYXX")),
    ];
    // With pshaw in answers this history is non-empty; force the empty-remaining path.
    let suggestion = suggest_guess_with_options(
        &lists,
        &[],
        &history,
        Some(3),
        false,
        false,
        OPENING_GUESS,
    )
    .expect("empty answer remaining should fall back to guess-pool matches");
    assert_eq!(suggestion.word, w("pshaw"));

    let pool = filter_by_history(&lists.guess_pool, &history);
    assert!(pool.contains(&w("pshaw")));
}
