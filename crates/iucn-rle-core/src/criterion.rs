//! Criterion and sub-criterion identifiers.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

/// An IUCN RLE sub-criterion.
///
/// The five criteria decompose into the 18 sub-criteria that an assessment
/// reports on. All are listed here even though only B1 and B2 currently have
/// threshold tables, because an assessment must be able to record a category for
/// any of them — including `NotEvaluated`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CriterionId {
    /// A1: decline in distribution over the past 50 years.
    A1,
    /// A2a: decline in distribution over any 50-year period including the future.
    A2a,
    /// A2b: decline in distribution over any 50-year period, future only.
    A2b,
    /// A3: decline in distribution since 1750.
    A3,
    /// B1: restricted extent of occurrence.
    B1,
    /// B2: restricted area of occupancy.
    B2,
    /// B3: very small number of locations.
    B3,
    /// C1: environmental degradation over the past 50 years.
    C1,
    /// C2a: environmental degradation over any 50-year period including the future.
    C2a,
    /// C2b: environmental degradation over any 50-year period, future only.
    C2b,
    /// C3: environmental degradation since 1750.
    C3,
    /// D1: disruption of biotic processes over the past 50 years.
    D1,
    /// D2a: disruption of biotic processes over any 50-year period including the future.
    D2a,
    /// D2b: disruption of biotic processes over any 50-year period, future only.
    D2b,
    /// D3: disruption of biotic processes since 1750.
    D3,
    /// E: quantitative risk analysis.
    E,
}

impl CriterionId {
    /// Every sub-criterion, in report order.
    pub const ALL: [Self; 16] = [
        Self::A1,
        Self::A2a,
        Self::A2b,
        Self::A3,
        Self::B1,
        Self::B2,
        Self::B3,
        Self::C1,
        Self::C2a,
        Self::C2b,
        Self::C3,
        Self::D1,
        Self::D2a,
        Self::D2b,
        Self::D3,
        Self::E,
    ];

    /// The identifier as written in assessments, such as `"A2b"`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::A1 => "A1",
            Self::A2a => "A2a",
            Self::A2b => "A2b",
            Self::A3 => "A3",
            Self::B1 => "B1",
            Self::B2 => "B2",
            Self::B3 => "B3",
            Self::C1 => "C1",
            Self::C2a => "C2a",
            Self::C2b => "C2b",
            Self::C3 => "C3",
            Self::D1 => "D1",
            Self::D2a => "D2a",
            Self::D2b => "D2b",
            Self::D3 => "D3",
            Self::E => "E",
        }
    }

    /// The parent criterion letter, such as `'B'`.
    #[must_use]
    pub const fn criterion(self) -> char {
        match self {
            Self::A1 | Self::A2a | Self::A2b | Self::A3 => 'A',
            Self::B1 | Self::B2 | Self::B3 => 'B',
            Self::C1 | Self::C2a | Self::C2b | Self::C3 => 'C',
            Self::D1 | Self::D2a | Self::D2b | Self::D3 => 'D',
            Self::E => 'E',
        }
    }
}

impl fmt::Display for CriterionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Returned when a string is not a recognised sub-criterion identifier.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("unknown IUCN RLE criterion: {0:?}")]
pub struct ParseCriterionError(pub String);

impl FromStr for CriterionId {
    type Err = ParseCriterionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.code().eq_ignore_ascii_case(s))
            .ok_or_else(|| ParseCriterionError(s.to_owned()))
    }
}
