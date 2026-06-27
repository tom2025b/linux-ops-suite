// ranking — the hybrid score: fuzzy match quality (primary) + frecency
// (frequency + recency, secondary) + a favorite bonus, with frecency CAPPED so
// it reorders among comparable matches but never lifts a poor textual match
// above a strong one. Pure functions over primitive inputs.
//
// Priority order: 1. fuzzy quality  2. frecency  3. alphabetical/mtime tiebreak.

/// Ceiling on the frecency boost — keeps a poor fuzzy match from winning on it.
const FRECENCY_CAP: f64 = 60.0;
/// Flat bonus when an entry is favorited.
const FAV_BONUS: f64 = 25.0;
/// Recency half-life in days: a run's recency weight halves every this many days.
const HALF_LIFE_DAYS: f64 = 14.0;
/// Shapes the approach to the cap: boost = cap*raw/(raw+k). Larger k = slower.
const FREQ_K: f64 = 3.0;
/// Multiplier on the boost when the last run failed (<1 demotes; 1 = off).
const FAIL_PENALTY: f64 = 0.85;

/// The run-history inputs ranking needs about one entry, assembled by the engine
/// (or the all-zero baseline for an entry with no history).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frecency {
    /// How many times the entry has been run.
    pub count: u64,
    /// Seconds since it was last run. `None` = never run.
    pub age_secs: Option<u64>,
    /// Whether the last run exited nonzero (failed).
    pub last_failed: bool,
}

impl Frecency {
    /// The "never run, not failed" baseline.
    pub const NONE: Frecency = Frecency {
        count: 0,
        age_secs: None,
        last_failed: false,
    };
}

/// Raw (uncapped) frecency: `ln(1+count) * recency`, recency halving every
/// `HALF_LIFE_DAYS`. Both factors must be high, so "ran 20× last year" loses to
/// "ran 5× this week".
pub fn frecency_raw(f: &Frecency) -> f64 {
    let Some(age) = f.age_secs else {
        return 0.0; // never run
    };
    if f.count == 0 {
        return 0.0;
    }
    let freq = (1.0 + f.count as f64).ln();
    let recency = 0.5_f64.powf(age as f64 / (HALF_LIFE_DAYS * 86_400.0));
    freq * recency
}

/// Squash the raw frecency into a bounded boost in `[0, cap)` and apply the
/// last-run-failed penalty: `cap * raw/(raw+k) * (fail? FAIL_PENALTY : 1)`.
pub fn frecency_boost(f: &Frecency) -> f64 {
    let raw = frecency_raw(f);
    if raw <= 0.0 {
        return 0.0;
    }
    let squashed = FRECENCY_CAP * raw / (raw + FREQ_K);
    if f.last_failed {
        squashed * FAIL_PENALTY
    } else {
        squashed
    }
}

/// The final ranking score for one entry: `fuzzy + frecency_boost + fav_bonus`.
pub fn score(fuzzy: f64, f: &Frecency, is_favorite: bool) -> f64 {
    let fav = if is_favorite { FAV_BONUS } else { 0.0 };
    fuzzy + frecency_boost(f) + fav
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn frecency(count: u64, age_secs: Option<u64>, last_failed: bool) -> Frecency {
        Frecency {
            count,
            age_secs,
            last_failed,
        }
    }

    #[test]
    fn never_run_or_zero_count_has_zero_frecency() {
        assert_eq!(frecency_raw(&Frecency::NONE), 0.0);
        assert_eq!(frecency_boost(&Frecency::NONE), 0.0);
        assert_eq!(frecency_raw(&frecency(0, Some(0), false)), 0.0);
    }

    #[test]
    fn recency_halves_at_one_half_life() {
        let r_now = frecency_raw(&frecency(10, Some(0), false));
        let r_hl = frecency_raw(&frecency(10, Some(14 * DAY), false));
        assert!(
            (r_hl / r_now - 0.5).abs() < 1e-9,
            "expected ~half: {r_hl}/{r_now}"
        );
    }

    #[test]
    fn more_frequent_and_more_recent_rank_higher() {
        // More frequent at equal recency.
        assert!(
            frecency_raw(&frecency(50, Some(DAY), false))
                > frecency_raw(&frecency(2, Some(DAY), false))
        );
        // More recent at equal frequency.
        assert!(
            frecency_raw(&frecency(10, Some(DAY), false))
                > frecency_raw(&frecency(10, Some(60 * DAY), false))
        );
    }

    #[test]
    fn frequency_has_diminishing_returns() {
        // ln is concave: 1→2 adds more than 100→101, so no script runs away.
        let raw_at = |c| frecency_raw(&frecency(c, Some(0), false));
        assert!(raw_at(2) - raw_at(1) > raw_at(101) - raw_at(100));
    }

    #[test]
    fn boost_is_capped_but_substantial() {
        let boost = frecency_boost(&frecency(1_000_000, Some(0), false));
        assert!(boost < FRECENCY_CAP, "boost {boost} must stay under cap");
        assert!(
            boost > FRECENCY_CAP * 0.5,
            "a heavily-used fresh entry gets a real boost"
        );
    }

    #[test]
    fn failed_last_run_demotes_boost() {
        let ok = frecency_boost(&frecency(20, Some(DAY), false));
        let failed = frecency_boost(&frecency(20, Some(DAY), true));
        assert!(failed < ok);
        assert!((failed / ok - FAIL_PENALTY).abs() < 1e-9);
    }

    #[test]
    fn score_sums_components_and_favorite_adds_the_bonus() {
        let f = frecency(5, Some(DAY), false);
        let plain = score(100.0, &f, false);
        let fav = score(100.0, &f, true);
        assert!((fav - plain - FAV_BONUS).abs() < 1e-9);
        assert!((plain - (100.0 + frecency_boost(&f))).abs() < 1e-9);
    }

    #[test]
    fn strong_fuzzy_match_beats_weak_match_with_max_frecency() {
        // Frecency reorders; it doesn't override match quality. A clearly stronger
        // text match (no history) beats a weaker, heavily-used, favorited one.
        let strong = score(200.0, &Frecency::NONE, false);
        let weak_but_used = score(120.0, &frecency(1_000, Some(0), false), true);
        assert!(strong > weak_but_used);
    }

    #[test]
    fn frecency_lifts_a_comparable_match_above_a_marginally_better_one() {
        // Within a close fuzzy band, the match you actually use wins.
        let slightly_better_unused = score(105.0, &Frecency::NONE, false);
        let comparable_but_used = score(100.0, &frecency(50, Some(DAY), false), false);
        assert!(comparable_but_used > slightly_better_unused);
    }
}
