use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    ops::Neg,
    rc::Rc,
};

use tracing::trace;

use crate::{
    dictionary::{Dictionary, Phrase},
    zhuyin::Syllable,
};

use super::{Break, ChineseSequence, ConversionEngine, Interval};

/// TODO: doc
#[derive(Debug)]
pub struct ChewingConversionEngine {
    dict: Rc<dyn Dictionary>,
}

impl ConversionEngine for ChewingConversionEngine {
    fn convert(&self, segment: &ChineseSequence) -> Vec<Interval> {
        if segment.syllables.is_empty() {
            return vec![];
        }
        let intervals = self.find_intervals(segment);
        self.find_best_path(segment.syllables.len(), intervals)
    }

    fn convert_next(&self, segment: &ChineseSequence, next: usize) -> Vec<Interval> {
        if segment.syllables.is_empty() {
            return vec![];
        }
        let mut graph = Graph::default();
        let paths = self.find_all_paths(&mut graph, segment, 0, segment.syllables.len(), None);
        let mut trimmed_paths = self.trim_paths(paths);
        trimmed_paths.sort();
        trimmed_paths
            .into_iter()
            .rev()
            .cycle()
            .nth(next)
            .map(|p| p.intervals)
            .expect("should have path")
            .into_iter()
            .map(|it| it.into())
            .collect()
    }
}

impl ChewingConversionEngine {
    /// TODO: doc
    pub fn new(dict: Rc<dyn Dictionary>) -> ChewingConversionEngine {
        ChewingConversionEngine { dict }
    }

    fn find_best_phrase(
        &self,
        start: usize,
        syllables: &[Syllable],
        selections: &[Interval],
        breaks: &[Break],
    ) -> Option<Rc<Phrase<'_>>> {
        let end = start + syllables.len();

        for br in breaks.iter() {
            if br.0 > start && br.0 < end {
                // There exists a break point that forbids connecting these
                // syllables.
                return None;
            }
        }

        let mut max_freq = 0;
        let mut best_phrase = None;
        'next_phrase: for phrase in self.dict.lookup_phrase(syllables) {
            // If there exists a user selected interval which is a
            // sub-interval of this phrase but the substring is
            // different then we can skip this phrase.
            for selection in selections.iter() {
                debug_assert!(!selection.phrase.is_empty());
                if start <= selection.start && end >= selection.end {
                    let offset = selection.start - start;
                    let len = selection.end - selection.start;
                    let substring: String =
                        phrase.as_str().chars().skip(offset).take(len).collect();
                    if substring != selection.phrase {
                        continue 'next_phrase;
                    }
                }
            }

            // If there are phrases that can satisfy all the constraints
            // then pick the one with highest frequency.
            if best_phrase.is_none() || phrase.freq() > max_freq {
                max_freq = phrase.freq();
                best_phrase = Some(Rc::new(phrase));
            }
        }

        best_phrase
    }
    fn find_intervals(&self, seq: &ChineseSequence) -> Vec<PossibleInterval<'_>> {
        let mut intervals = vec![];
        for begin in 0..seq.syllables.len() {
            for end in begin..=seq.syllables.len() {
                if let Some(phrase) = self.find_best_phrase(
                    begin,
                    &seq.syllables[begin..end],
                    &seq.selections,
                    &seq.breaks,
                ) {
                    intervals.push(PossibleInterval {
                        start: begin,
                        end,
                        phrase,
                    });
                }
            }
        }
        intervals
    }
    /// Calculate the best path with dynamic programming.
    ///
    /// Assume P(x,y) is the highest score phrasing result from x to y. The
    /// following is formula for P(x,y):
    ///
    /// P(x,y) = MAX( P(x,y-1)+P(y-1,y), P(x,y-2)+P(y-2,y), ... )
    ///
    /// While P(x,y-1) is stored in highest_score array, and P(y-1,y) is
    /// interval end at y. In this formula, x is always 0.
    ///
    /// The format of highest_score array is described as following:
    ///
    /// highest_score[0] = P(0,0)
    /// highest_score[1] = P(0,1)
    /// ...
    /// highest_score[y-1] = P(0,y-1)
    fn find_best_path(&self, len: usize, mut intervals: Vec<PossibleInterval<'_>>) -> Vec<Interval> {
        let mut highest_score = vec![PossiblePath::default(); len + 1];

        // The interval shall be sorted by the increase order of end.
        intervals.sort_by(|a, b| a.end.cmp(&b.end));

        for interval in intervals.into_iter() {
            let start = interval.start;
            let end = interval.end;

            let mut candidate_path = highest_score[start].clone();
            candidate_path.intervals.push(interval);

            if highest_score[end].score() < candidate_path.score() {
                highest_score[end] = candidate_path;
            }
        }

        highest_score
            .pop()
            .expect("highest_score has at least one element")
            .intervals
            .into_iter()
            .map(|interval| interval.into())
            .collect()
    }

    fn find_all_paths<'g>(
        &'g self,
        graph: &mut Graph<'g>,
        sequence: &ChineseSequence,
        start: usize,
        target: usize,
        prefix: Option<PossiblePath<'g>>,
    ) -> Vec<PossiblePath<'g>> {
        if start == target {
            return vec![prefix.expect("should have prefix")];
        }
        let mut result = vec![];
        for end in start..=target {
            let entry = graph.entry((start, end));
            if let Some(phrase) = entry.or_insert_with(|| {
                self.find_best_phrase(
                    start,
                    &sequence.syllables[start..end],
                    &sequence.selections,
                    &sequence.breaks,
                )
            }) {
                let mut prefix = prefix.clone().unwrap_or_default();
                prefix.intervals.push(PossibleInterval {
                    start,
                    end,
                    phrase: phrase.clone(),
                });
                result.append(&mut self.find_all_paths(graph, sequence, end, target, Some(prefix)));
            }
        }
        result
    }

    /// Trim some paths that were part of other paths
    ///
    /// Ported from original C implementation, but the original algorithm seems wrong.
    fn trim_paths<'a>(&self, paths: Vec<PossiblePath<'a>>) -> Vec<PossiblePath<'a>> {
        let mut trimmed_paths: Vec<PossiblePath<'_>> = vec![];
        for candidate in paths.into_iter() {
            trace!("Trim check {}", candidate);
            let mut drop_candidate = false;
            let mut keeper = vec![];
            for p in trimmed_paths.into_iter() {
                if drop_candidate || p.contains(&candidate) {
                    drop_candidate = true;
                    trace!("  Keep {}", p);
                    keeper.push(p);
                    continue;
                }
                if candidate.contains(&p) {
                    trace!("  Drop {}", p);
                    continue;
                }
                trace!("  Keep {}", p);
                keeper.push(p);
            }
            if !drop_candidate {
                trace!("  Keep {}", candidate);
                keeper.push(candidate);
            }
            trimmed_paths = keeper;
        }
        trimmed_paths
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PossibleInterval<'a> {
    start: usize,
    end: usize,
    phrase: Rc<Phrase<'a>>,
}

impl PossibleInterval<'_> {
    fn contains(&self, other: &PossibleInterval<'_>) -> bool {
        self.start <= other.start && self.end >= other.end
    }
    fn len(&self) -> usize {
        self.end - self.start
    }
}

impl From<PossibleInterval<'_>> for Interval {
    fn from(value: PossibleInterval<'_>) -> Self {
        Interval {
            start: value.start,
            end: value.end,
            phrase: value.phrase.to_string(),
        }
    }
}

#[derive(Default, Clone, Eq)]
struct PossiblePath<'a> {
    intervals: Vec<PossibleInterval<'a>>,
}

impl Debug for PossiblePath<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PossiblePath")
            .field("score()", &self.score())
            .field("intervals", &self.intervals)
            .finish()
    }
}

impl PossiblePath<'_> {
    fn score(&self) -> i32 {
        let mut score = 0;
        score += 1000 * self.rule_largest_sum();
        score += 1000 * self.rule_largest_avgwordlen();
        score += 100 * self.rule_smallest_lenvariance();
        score += self.rule_largest_freqsum();
        score
    }

    /// Copied from IsRecContain to trim some paths
    fn contains(&self, other: &Self) -> bool {
        let mut big = 0;
        for sml in 0..other.intervals.len() {
            loop {
                if big < self.intervals.len()
                    && self.intervals[big].start < other.intervals[sml].end
                {
                    if self.intervals[big].contains(&other.intervals[sml]) {
                        break;
                    }
                } else {
                    return false;
                }
                big += 1;
            }
        }
        true
    }

    fn rule_largest_sum(&self) -> i32 {
        let mut score = 0;
        for interval in &self.intervals {
            score += interval.end - interval.start;
        }
        score as i32
    }

    fn rule_largest_avgwordlen(&self) -> i32 {
        if self.intervals.is_empty() {
            return 0;
        }
        // Constant factor 6=1*2*3, to keep value as integer
        6 * self.rule_largest_sum()
            / i32::try_from(self.intervals.len()).expect("number of intervals should be small")
    }

    fn rule_smallest_lenvariance(&self) -> i32 {
        let len = self.intervals.len();
        let mut score = 0;
        // kcwu: heuristic? why variance no square function?
        for i in 0..len {
            for j in i + 1..len {
                let interval_1 = &self.intervals[i];
                let interval_2 = &self.intervals[j];
                score += interval_1.len().abs_diff(interval_2.len());
            }
        }
        i32::try_from(score).expect("score should fit in i32").neg()
    }

    fn rule_largest_freqsum(&self) -> i32 {
        let mut score = 0;
        for interval in &self.intervals {
            let reduction_factor = if interval.len() == 1 { 512 } else { 1 };
            score += interval.phrase.freq() / reduction_factor;
        }
        i32::try_from(score).expect("score should fit in i32")
    }
}

impl PartialEq for PossiblePath<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.score() == other.score()
    }
}

impl PartialOrd for PossiblePath<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.score().partial_cmp(&other.score())
    }
}

impl Ord for PossiblePath<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score().cmp(&other.score())
    }
}

impl Display for PossiblePath<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#PossiblePath({}", self.score())?;
        for interval in &self.intervals {
            write!(
                f,
                " ({} {} '{})",
                interval.start, interval.end, interval.phrase
            )?;
        }
        write!(f, ")")?;
        Ok(())
    }
}

type Graph<'a> = HashMap<(usize, usize), Option<Rc<Phrase<'a>>>>;

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, rc::Rc};

    use crate::{
        conversion::{Break, ChineseSequence, ConversionEngine, Interval},
        dictionary::{Dictionary, Phrase},
        syl,
        zhuyin::Bopomofo::*,
    };

    use super::{ChewingConversionEngine, PossibleInterval, PossiblePath};

    fn test_dictionary() -> Rc<dyn Dictionary> {
        Rc::new(HashMap::from([
            (vec![syl![G, U, O, TONE2]], vec![("國", 1).into()]),
            (vec![syl![M, I, EN, TONE2]], vec![("民", 1).into()]),
            (vec![syl![D, A, TONE4]], vec![("大", 1).into()]),
            (vec![syl![H, U, EI, TONE4]], vec![("會", 1).into()]),
            (vec![syl![D, AI, TONE4]], vec![("代", 1).into()]),
            (vec![syl![B, I, AU, TONE3]], vec![("表", 1).into()]),
            (
                vec![syl![G, U, O, TONE2], syl![M, I, EN, TONE2]],
                vec![("國民", 200).into()],
            ),
            (
                vec![syl![D, A, TONE4], syl![H, U, EI, TONE4]],
                vec![("大會", 200).into()],
            ),
            (
                vec![syl![D, AI, TONE4], syl![B, I, AU, TONE3]],
                vec![("代表", 200).into(), ("戴錶", 100).into()],
            ),
            (vec![syl![X, I, EN]], vec![("心", 1).into()]),
            (
                vec![syl![K, U, TONE4], syl![I, EN]],
                vec![("庫音", 300).into()],
            ),
            (
                vec![syl![X, I, EN], syl![K, U, TONE4], syl![I, EN]],
                vec![("新酷音", 200).into()],
            ),
            (
                vec![syl![C, E, TONE4], syl![SH, TONE4], syl![I, TONE2]],
                vec![("測試儀", 42).into()],
            ),
            (
                vec![syl![C, E, TONE4], syl![SH, TONE4]],
                vec![("測試", 9318).into()],
            ),
            (
                vec![syl![I, TONE2], syl![X, I, A, TONE4]],
                vec![("一下", 10576).into()],
            ),
            (vec![syl![X, I, A, TONE4]], vec![("下", 10576).into()]),
        ]))
    }

    #[test]
    fn convert_empty_sequence() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![],
            selections: vec![],
            breaks: vec![],
        };
        assert_eq!(Vec::<Interval>::new(), engine.convert(&sequence));
    }

    #[test]
    fn convert_simple_chinese_sequence() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![
                syl![G, U, O, TONE2],
                syl![M, I, EN, TONE2],
                syl![D, A, TONE4],
                syl![H, U, EI, TONE4],
                syl![D, AI, TONE4],
                syl![B, I, AU, TONE3],
            ],
            selections: vec![],
            breaks: vec![],
        };
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 2,
                    phrase: "國民".to_string()
                },
                Interval {
                    start: 2,
                    end: 4,
                    phrase: "大會".to_string()
                },
                Interval {
                    start: 4,
                    end: 6,
                    phrase: "代表".to_string()
                },
            ],
            engine.convert(&sequence)
        );
    }

    #[test]
    fn convert_chinese_sequence_with_breaks() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![
                syl![G, U, O, TONE2],
                syl![M, I, EN, TONE2],
                syl![D, A, TONE4],
                syl![H, U, EI, TONE4],
                syl![D, AI, TONE4],
                syl![B, I, AU, TONE3],
            ],
            selections: vec![],
            breaks: vec![Break(1), Break(5)],
        };
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 1,
                    phrase: "國".to_string()
                },
                Interval {
                    start: 1,
                    end: 2,
                    phrase: "民".to_string()
                },
                Interval {
                    start: 2,
                    end: 4,
                    phrase: "大會".to_string()
                },
                Interval {
                    start: 4,
                    end: 5,
                    phrase: "代".to_string()
                },
                Interval {
                    start: 5,
                    end: 6,
                    phrase: "表".to_string()
                },
            ],
            engine.convert(&sequence)
        );
    }

    #[test]
    fn convert_chinese_sequence_with_good_selection() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![
                syl![G, U, O, TONE2],
                syl![M, I, EN, TONE2],
                syl![D, A, TONE4],
                syl![H, U, EI, TONE4],
                syl![D, AI, TONE4],
                syl![B, I, AU, TONE3],
            ],
            selections: vec![Interval {
                start: 4,
                end: 6,
                phrase: "戴錶".to_string(),
            }],
            breaks: vec![],
        };
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 2,
                    phrase: "國民".to_string()
                },
                Interval {
                    start: 2,
                    end: 4,
                    phrase: "大會".to_string()
                },
                Interval {
                    start: 4,
                    end: 6,
                    phrase: "戴錶".to_string()
                },
            ],
            engine.convert(&sequence)
        );
    }

    #[test]
    fn convert_chinese_sequence_with_substring_selection() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![syl![X, I, EN], syl![K, U, TONE4], syl![I, EN]],
            selections: vec![Interval {
                start: 1,
                end: 3,
                phrase: "酷音".to_string(),
            }],
            breaks: vec![],
        };
        assert_eq!(
            vec![Interval {
                start: 0,
                end: 3,
                phrase: "新酷音".to_string()
            },],
            engine.convert(&sequence)
        );
    }

    #[test]
    fn convert_cycle_alternatives() {
        let dict = test_dictionary();
        let engine = ChewingConversionEngine::new(dict);
        let sequence = ChineseSequence {
            syllables: vec![
                syl![C, E, TONE4],
                syl![SH, TONE4],
                syl![I, TONE2],
                syl![X, I, A, TONE4],
            ],
            selections: vec![],
            breaks: vec![],
        };
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 2,
                    phrase: "測試".to_string()
                },
                Interval {
                    start: 2,
                    end: 4,
                    phrase: "一下".to_string()
                }
            ],
            engine.convert_next(&sequence, 0)
        );
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 3,
                    phrase: "測試儀".to_string()
                },
                Interval {
                    start: 3,
                    end: 4,
                    phrase: "下".to_string()
                }
            ],
            engine.convert_next(&sequence, 1)
        );
        assert_eq!(
            vec![
                Interval {
                    start: 0,
                    end: 2,
                    phrase: "測試".to_string()
                },
                Interval {
                    start: 2,
                    end: 4,
                    phrase: "一下".to_string()
                }
            ],
            engine.convert_next(&sequence, 2)
        );
    }

    #[test]
    fn possible_path_contains() {
        let path_1 = PossiblePath {
            intervals: vec![
                PossibleInterval {
                    start: 0,
                    end: 2,
                    phrase: Phrase::new("測試", 0).into(),
                },
                PossibleInterval {
                    start: 2,
                    end: 4,
                    phrase: Phrase::new("一下", 0).into(),
                },
            ],
        };
        let path_2 = PossiblePath {
            intervals: vec![
                PossibleInterval {
                    start: 0,
                    end: 2,
                    phrase: Phrase::new("測試", 0).into(),
                },
                PossibleInterval {
                    start: 2,
                    end: 3,
                    phrase: Phrase::new("遺", 0).into(),
                },
                PossibleInterval {
                    start: 3,
                    end: 4,
                    phrase: Phrase::new("下", 0).into(),
                },
            ],
        };
        assert!(path_1.contains(&path_2));
    }
}
