use crate::{
    core::oids::Oids,
    helpers::{palette::Theme, symbols::SymbolTheme},
};
use chrono::{Datelike, NaiveDate};
use chrono::{TimeZone, Utc};
use gix::prelude::FindExt;
use ratatui::{style::Style, text::Span};

pub const WEEKS: usize = 53;
pub const DAYS: usize = 7;
const TOTAL_DAYS: usize = WEEKS * DAYS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeatmapCounts {
    today: NaiveDate,
    counts: [usize; TOTAL_DAYS],
}

impl Default for HeatmapCounts {
    fn default() -> Self {
        Self { today: Utc::now().date_naive(), counts: [0usize; TOTAL_DAYS] }
    }
}

impl HeatmapCounts {
    pub fn add_commit_seconds(&mut self, seconds: i64) {
        if let DateBucket::Count(days_ago) = bucket_seconds(self.today, seconds) {
            self.counts[days_ago] += 1;
        }
    }

    pub fn build(&self) -> [[usize; WEEKS]; DAYS] {
        build_heatmap_from_counts_for_day(self.counts, self.today)
    }
}

pub fn commits_per_day(repo: &gix::Repository, oids: impl IntoIterator<Item = gix::ObjectId>) -> [usize; TOTAL_DAYS] {
    let mut object_buf = Vec::new();
    counts_from_commit_seconds(oids.into_iter().filter_map(|oid| {
        object_buf.clear();
        commit_seconds(repo, oid, &mut object_buf)
    }))
}

fn commit_seconds(repo: &gix::Repository, oid: gix::ObjectId, object_buf: &mut Vec<u8>) -> Option<i64> {
    Some(repo.objects.find_commit(oid.as_ref(), object_buf).ok()?.time().ok()?.seconds)
}

fn counts_from_commit_seconds(seconds: impl IntoIterator<Item = i64>) -> [usize; TOTAL_DAYS] {
    counts_from_commit_seconds_for_day(seconds, Utc::now().date_naive())
}

fn counts_from_commit_seconds_for_day(seconds: impl IntoIterator<Item = i64>, today: NaiveDate) -> [usize; TOTAL_DAYS] {
    let mut counts = [0usize; TOTAL_DAYS];

    for seconds in seconds {
        match bucket_seconds(today, seconds) {
            DateBucket::Count(days_ago) => counts[days_ago] += 1,
            DateBucket::Future => continue,
            DateBucket::BeforeWindow => break,
        }
    }

    counts
}

pub fn empty_heatmap() -> [[usize; WEEKS]; DAYS] {
    [[0usize; WEEKS]; DAYS]
}

pub fn build_heatmap(repo: &gix::Repository, oids: impl IntoIterator<Item = gix::ObjectId>) -> [[usize; WEEKS]; DAYS] {
    build_heatmap_from_counts(commits_per_day(repo, oids))
}

pub fn build_heatmap_from_sorted_aliases(repo: &gix::Repository, oids: &Oids) -> [[usize; WEEKS]; DAYS] {
    build_heatmap_from_counts(commits_per_day(repo, oids.get_sorted_aliases().iter().map(|alias| *oids.get_gix_oid_by_alias(*alias))))
}

fn build_heatmap_from_counts(counts: [usize; TOTAL_DAYS]) -> [[usize; WEEKS]; DAYS] {
    build_heatmap_from_counts_for_day(counts, Utc::now().date_naive())
}

fn build_heatmap_from_counts_for_day(counts: [usize; TOTAL_DAYS], today: NaiveDate) -> [[usize; WEEKS]; DAYS] {
    let weekday_today = today.weekday().num_days_from_monday() as usize;
    let mut grid = [[0usize; WEEKS]; DAYS];

    for (cell, count) in heatmap_cells(weekday_today, counts) {
        grid[cell.day][cell.week] = count;
    }

    grid
}

fn bucket_seconds(today: NaiveDate, seconds: i64) -> DateBucket {
    let Some(commit_date) = Utc.timestamp_opt(seconds, 0).single().map(|date| date.date_naive()) else {
        return DateBucket::Future;
    };
    let days_ago = today.signed_duration_since(commit_date).num_days();

    if days_ago < 0 {
        DateBucket::Future
    } else if days_ago >= TOTAL_DAYS as i64 {
        DateBucket::BeforeWindow
    } else {
        DateBucket::Count(days_ago as usize)
    }
}

fn heatmap_cells(count_weekday: usize, counts: [usize; TOTAL_DAYS]) -> impl Iterator<Item = (HeatmapCell, usize)> {
    counts.into_iter().enumerate().filter_map(move |(days_ago, count)| heatmap_cell(count_weekday, days_ago).map(|cell| (cell, count)))
}

fn heatmap_cell(weekday_today: usize, days_ago: usize) -> Option<HeatmapCell> {
    let offset = 6 - weekday_today;
    let logical = days_ago + offset;
    let week = logical / 7;
    (week < WEEKS).then(|| HeatmapCell { day: weekday_for_age(weekday_today, days_ago), week: WEEKS - 1 - week })
}

fn weekday_for_age(weekday_today: usize, days_ago: usize) -> usize {
    (weekday_today + DAYS - (days_ago % DAYS)) % DAYS
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct HeatmapCell {
    day: usize,
    week: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DateBucket {
    Count(usize),
    Future,
    BeforeWindow,
}

pub fn heat_cell(count: usize, theme: &Theme, symbols: &SymbolTheme) -> Span<'static> {
    let (character, color) = match count {
        0 => (symbols.heatmap.cell(count), Some(theme.COLOR_TEXT)),
        _ => (symbols.heatmap.cell(count), Some(theme.COLOR_GRASS)),
    };
    let style = color.map_or_else(Style::default, |c| Style::default().fg(c));
    Span::styled(format!("{:>2}", character), style)
}

#[cfg(test)]
#[path = "../tests/helpers/heatmap.rs"]
mod tests;
