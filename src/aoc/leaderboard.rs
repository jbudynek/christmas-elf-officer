use crate::utils::challenge_release_time;
use chrono::naive::NaiveDateTime;
use chrono::{DateTime, Duration, Utc};
use itertools::Itertools;
use scraper::{Node, Selector};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::iter::Iterator;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ProblemPart {
    FIRST,
    SECOND,
}

#[derive(Debug)]
pub struct LeaderboardStatistics {
    pub p1_time_fast: Option<Duration>,
    pub p1_time_slow: Option<Duration>,
    pub p2_time_fast: Option<Duration>,
    pub p2_time_slow: Option<Duration>,
    // We also retrieve final rank (part 2) in addition of delta time
    pub delta_fast: Option<(Duration, u8)>,
    pub delta_slow: Option<(Duration, u8)>,
}

// Puzzle completion events parsed from AoC API.
// Year and day fields match corresponding components of DateTime<Utc>.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Solution {
    pub timestamp: DateTime<Utc>,
    pub year: i32,
    pub day: u8,
    pub part: ProblemPart,
    pub id: Identifier,
    pub rank: Option<u8>,
}

// unique identifier for a participant on this leaderboard
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct Identifier {
    pub name: String,
    pub numeric: u64,
    pub global_score: u64,
}

type SolutionVec = Vec<Solution>;

#[derive(Debug)]
pub struct Leaderboard(SolutionVec);

#[derive(Debug)]
pub struct ScrapedLeaderboard {
    pub timestamp: chrono::DateTime<Utc>,
    pub leaderboard: Leaderboard,
}

impl fmt::Display for ProblemPart {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProblemPart::FIRST => {
                write!(f, "1")
            }
            ProblemPart::SECOND => {
                write!(f, "2")
            }
        }
    }
}

impl ProblemPart {
    pub fn from(input: usize) -> Self {
        match input {
            1 => ProblemPart::FIRST,
            2 => ProblemPart::SECOND,
            // only two parts for each problem
            _ => unreachable!(),
        }
    }
}

impl Solution {
    pub fn from_html(
        entry: scraper::element_ref::ElementRef,
        year: i32,
        day: u8,
        part: ProblemPart,
    ) -> Option<Self> {
        let rank_selector = Selector::parse(r#".leaderboard-position"#).unwrap();
        let time_selector = Selector::parse(r#".leaderboard-time"#).unwrap();

        let id = match entry.value().attr("data-user-id") {
            Some(id) => id.parse::<u64>().ok(),
            None => None,
        };

        // Depending on whether users have declared their github, are sponsors, etc ... the name
        // will be accessible in different possible DOM hierarchy layouts.
        let name = entry
            .children()
            .filter_map(|node| match node.value() {
                Node::Text(text) => Some(text.trim()),
                Node::Element(el) => match el.name() {
                    // Name wrapped into <a> tags to link to user's github.
                    "a" => {
                        let text = node.last_child().unwrap().value();
                        let text = text.as_text().unwrap().trim();
                        // We ignore <a> tags related to (AoC++) or (Sponsor) labels.
                        match (text.starts_with("("), text.ends_with(")")) {
                            (false, false) => Some(text),
                            (_, _) => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .last();

        let rank = match entry.select(&rank_selector).next() {
            Some(text) => match text.text().next() {
                Some(t) => t
                    .split(")")
                    .next()
                    .map_or(None, |rank| rank.trim().parse::<u8>().ok()),
                None => None,
            },
            None => None,
        };

        let timestamp = match entry.select(&time_selector).next() {
            Some(t) => t
                .text()
                .filter_map(|time| {
                    let with_year = format!("{} {}", year, time);
                    let naive_datetime =
                        NaiveDateTime::parse_from_str(&with_year, "%Y %b %d  %H:%M:%S").ok();
                    naive_datetime
                })
                .map(|d| DateTime::<Utc>::from_utc(d, Utc) + Duration::hours(6))
                .last(),
            None => None,
        };

        match (id, name, rank, timestamp) {
            (Some(id), _, Some(rank), Some(timestamp)) => Some(Solution {
                id: Identifier {
                    // Name of anonymous user will be None
                    name: name
                        .map_or(format!("anonymous user #{}", id), |n| n.to_string())
                        .to_string(),
                    numeric: id,
                    // We won't use it
                    global_score: 0,
                },
                rank: Some(rank),
                part,
                year,
                day,
                timestamp,
            }),
            _ => None,
        }
    }
}

impl Leaderboard {
    pub fn new() -> Leaderboard {
        Leaderboard(SolutionVec::new())
    }

    /// Members => (unordered) stars
    fn solutions_per_member(&self) -> HashMap<&Identifier, Vec<&Solution>> {
        self.iter().into_group_map_by(|a| &a.id)
    }

    fn solutions_per_challenge(&self) -> HashMap<(u8, ProblemPart), Vec<&Solution>> {
        self.iter().into_group_map_by(|a| (a.day, a.part))
    }

    pub fn members_ids(&self) -> Vec<u64> {
        self.solutions_per_member()
            .iter()
            .map(|(id, _)| id.numeric)
            .collect::<Vec<u64>>()
    }

    pub fn get_member_by_id(&self, id: u64) -> Option<&Identifier> {
        self.solutions_per_member()
            .into_iter()
            .find_map(|(m_id, _)| match m_id.numeric == id {
                true => Some(m_id),
                false => None,
            })
    }

    fn standings_per_challenge(&self) -> HashMap<(u8, ProblemPart), Vec<&Identifier>> {
        self.solutions_per_challenge()
            .into_iter()
            .map(|(challenge, solutions)| {
                (
                    challenge,
                    solutions
                        .into_iter()
                        // sort solutions chronologically by timestamp
                        .sorted_unstable()
                        // retrieve author of the solution
                        .map(|s| &s.id)
                        .collect(),
                )
            })
            .collect::<HashMap<(u8, ProblemPart), Vec<&Identifier>>>()
    }

    fn daily_scores_per_member(&self) -> HashMap<&Identifier, [usize; 25]> {
        // Max point earned for each star is number of members in leaderboard
        let n_members = self.solutions_per_member().len();

        let standings_per_challenge = self.standings_per_challenge();
        standings_per_challenge
            .iter()
            .fold(HashMap::new(), |mut acc, ((day, _), star_rank)| {
                star_rank.iter().enumerate().for_each(|(rank, id)| {
                    let star_score = n_members - rank;
                    let day_scores = acc.entry(*id).or_insert([0; 25]);
                    day_scores[(*day - 1) as usize] += star_score;
                });
                acc
            })
    }

    fn local_scores_per_member(&self) -> HashMap<&Identifier, usize> {
        self.daily_scores_per_member()
            .iter()
            .map(|(id, daily_scores)| (*id, daily_scores.iter().sum()))
            .collect()
    }

    pub fn compute_diffs(&self, current_leaderboard: &Leaderboard) -> Vec<&Solution> {
        let current_solutions = current_leaderboard
            .iter()
            .map(|s| (s.id.numeric, s.day, s.part));

        self.iter()
            // The curent_solutions iterator needs to be cloned as .contains() consumes it partially
            // (or totally if no match found)
            .filter(|s| {
                !current_solutions
                    .clone()
                    .contains(&(s.id.numeric, s.day, s.part))
            })
            .collect()
    }

    pub fn standings_by_local_score(&self) -> Vec<(String, usize)> {
        let scores = self.local_scores_per_member();

        scores
            .into_iter()
            .sorted_by_key(|x| Reverse(x.1))
            .map(|(id, score)| (id.name.clone(), score))
            .collect::<Vec<(String, usize)>>()
    }

    pub fn standings_by_number_of_stars(&self) -> Vec<(String, usize)> {
        let stars = self.solutions_per_member();

        stars
            .into_iter()
            .map(|(id, stars)| {
                (
                    id.name.clone(),
                    stars.len(),
                    // Get the timestamp of the last earned star
                    stars.into_iter().sorted_unstable().last(),
                )
            })
            // Sort by number of star (reverse) then by most recent star on equality
            .sorted_by_key(|x| (Reverse(x.1), x.2))
            .map(|(name, n_stars, _)| (name, n_stars))
            .collect::<Vec<(String, usize)>>()
    }

    pub fn standings_by_global_score(&self) -> Vec<(String, u64)> {
        self.solutions_per_member()
            .iter()
            .filter(|(id, _)| id.global_score > 0)
            .map(|(id, _)| (id.name.clone(), id.global_score))
            .sorted_by_key(|h| Reverse(h.1))
            .collect::<Vec<(String, u64)>>()
    }

    pub fn standings_by_local_score_for_day(&self, day: usize) -> Vec<(String, usize)> {
        self.daily_scores_per_member()
            .iter()
            .map(|(id, daily_scores)| (id.name.clone(), daily_scores[day - 1]))
            .filter(|(_, score)| *score > 0)
            .sorted_by_key(|m| Reverse(m.1))
            .collect::<Vec<(String, usize)>>()
    }

    // ranking by time between part 1 and part 2 completions
    pub fn standings_by_delta_for_day(&self, day: u8) -> Vec<(String, Duration)> {
        self.solutions_per_member()
            .into_iter()
            .filter_map(|(id, solutions)| {
                let solutions_for_day = solutions.iter().filter(|s| s.day == day);
                match solutions_for_day.clone().count() {
                    0 | 1 => None,
                    2 => {
                        let mut ordered_parts =
                            solutions_for_day.sorted_by_key(|s| s.timestamp).tuples();
                        let (first, second) = ordered_parts.next().unwrap();
                        Some((id.name.clone(), second.timestamp - first.timestamp))
                    }
                    _ => unreachable!(),
                }
            })
            .sorted_by_key(|r| r.1)
            .collect::<Vec<(String, Duration)>>()
    }
}

impl Deref for Leaderboard {
    type Target = SolutionVec;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Leaderboard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ScrapedLeaderboard {
    pub fn new() -> ScrapedLeaderboard {
        ScrapedLeaderboard {
            timestamp: Utc::now(),
            leaderboard: Leaderboard::new(),
        }
    }

    pub fn is_count_equal_to(&self, n: usize) -> bool {
        self.leaderboard.len() == n
    }

    pub fn statistics(&self, year: i32, day: u8) -> LeaderboardStatistics {
        // Separate entries into part1/part2
        let data = self
            .leaderboard
            .iter()
            .filter(|e| e.year == year && e.day == day)
            .into_group_map_by(|entry| entry.part);

        // Make sure it's chronologically ordered.
        let mut part_1 = data.get(&ProblemPart::FIRST).unwrap().clone();
        let mut part_2 = data.get(&ProblemPart::SECOND).unwrap().clone();
        part_1.sort_by_key(|e| e.timestamp);
        part_2.sort_by_key(|e| e.timestamp);

        // Prepare iterators to retrieve values.
        let mut part_1 = part_1.iter();
        let mut part_2 = part_2.iter();

        let challenge_start_time = challenge_release_time(year, day);

        // Needed for computation of deltas for members who only scored one part of the global
        // leaderboard that day.
        let (max_time_for_first_part, max_time_for_second_part) = self.leaderboard.iter().fold(
            (DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MIN_UTC),
            |mut acc, entry| match entry.part {
                ProblemPart::FIRST => {
                    if entry.timestamp > acc.0 {
                        acc.0 = entry.timestamp
                    };
                    acc
                }
                ProblemPart::SECOND => {
                    if entry.timestamp > acc.1 {
                        acc.1 = entry.timestamp
                    };
                    acc
                }
            },
        );

        // Compute deltas
        let by_id = self
            .leaderboard
            .iter()
            .filter(|e| e.rank.is_some())
            .into_group_map_by(|e| e.id.numeric);

        let mut sorted_deltas = by_id
            .into_iter()
            .map(|(_id, entries)| {
                match entries.len() {
                    1 => {
                        // unwrap is safe as len == 1
                        let entry = entries.last().unwrap();
                        match entry.part {
                            ProblemPart::FIRST => {
                                // Scored only first part, second part overtime.
                                // Duration is > (max second part - part.1), so we'll add 1 to the
                                // diff as we have no way to know exactly
                                (
                                    max_time_for_second_part - entry.timestamp
                                        + Duration::seconds(1),
                                    101,
                                )
                            }
                            ProblemPart::SECOND => {
                                // Overtimed on first part, but came back strong to score second part
                                // Duration is > (part.1, - max first part). We'll substract 1 sec.
                                (
                                    entry.timestamp
                                        - max_time_for_first_part
                                        - Duration::seconds(1),
                                    entry.rank.unwrap(),
                                )
                            }
                        }
                    }
                    2 => {
                        let mut sorted = entries.into_iter().sorted_by_key(|e| e.timestamp);
                        // unwrap are safe as len == 2
                        let (p1, p2) = (sorted.next().unwrap(), sorted.next().unwrap());
                        (p2.timestamp - p1.timestamp, p2.rank.unwrap())
                    }
                    _ => unreachable!(),
                }
            })
            .filter(|(_duration, rank)| rank <= &100)
            .sorted();

        let statistics = LeaderboardStatistics {
            p1_time_fast: part_1
                .next()
                .map_or(None, |e| Some(e.timestamp - challenge_start_time)),
            p1_time_slow: part_1
                .last()
                .map_or(None, |e| Some(e.timestamp - challenge_start_time)),
            p2_time_fast: part_2
                .next()
                .map_or(None, |e| Some(e.timestamp - challenge_start_time)),
            p2_time_slow: part_2
                .last()
                .map_or(None, |e| Some(e.timestamp - challenge_start_time)),
            delta_fast: sorted_deltas.next(),
            delta_slow: sorted_deltas.last(),
        };
        statistics
    }

    pub fn check_for_private_members(
        &self,
        private_leaderboard: &Leaderboard,
    ) -> Vec<(Identifier, ProblemPart)> {
        let private_members_ids = private_leaderboard.members_ids();
        let heroes = self
            .leaderboard
            .iter()
            .filter(|entry| private_members_ids.contains(&entry.id.numeric))
            .map(|entry| {
                (
                    private_leaderboard
                        .get_member_by_id(entry.id.numeric)
                        // we can safely unwrap as if it enters the map there is a match
                        .unwrap()
                        .clone(),
                    entry.part,
                )
            })
            .collect::<Vec<(Identifier, ProblemPart)>>();
        heroes
    }
}