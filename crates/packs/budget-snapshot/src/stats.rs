//! Statistics over the samples of one handler.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::resource::{Resource, Sample};

/// The two numbers a snapshot keeps per resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    /// Largest value seen.
    pub max: i64,
    /// Lower median: `sorted[(n - 1) / 2]`. Integer only, so the file has no
    /// floating-point formatting to differ between machines.
    pub p50: i64,
}

impl Stat {
    /// `max` and `p50` of `values`, or `None` for no values.
    pub fn of(values: &mut [i64]) -> Option<Stat> {
        values.sort_unstable();
        let max = *values.last()?;
        let p50 = values[(values.len() - 1) / 2];
        Some(Stat { max, p50 })
    }
}

/// What a snapshot records about one handler.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerStats {
    /// Number of successful, correctly attributed calls measured.
    pub samples: u64,
    /// Statistics per resource, keyed by [`Resource::name`].
    #[serde(flatten)]
    pub resources: BTreeMap<String, Stat>,
}

impl HandlerStats {
    /// Statistics over `samples`, or `None` when there are none.
    pub fn from_samples(samples: &[Sample]) -> Option<HandlerStats> {
        if samples.is_empty() {
            return None;
        }
        let mut resources = BTreeMap::new();
        for resource in Resource::ALL {
            let mut values: Vec<i64> = samples.iter().map(|s| s.get(resource)).collect();
            resources.insert(resource.name().to_owned(), Stat::of(&mut values)?);
        }
        Some(HandlerStats {
            samples: samples.len() as u64,
            resources,
        })
    }

    /// The statistics of one resource.
    pub fn get(&self, resource: Resource) -> Option<Stat> {
        self.resources.get(resource.name()).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_is_the_lower_middle_element() {
        assert_eq!(Stat::of(&mut [5]), Some(Stat { max: 5, p50: 5 }));
        assert_eq!(Stat::of(&mut [9, 1, 5]), Some(Stat { max: 9, p50: 5 }));
        assert_eq!(Stat::of(&mut [4, 1, 3, 2]), Some(Stat { max: 4, p50: 2 }));
        assert_eq!(Stat::of(&mut []), None);
    }

    #[test]
    fn from_samples_covers_every_resource() {
        let samples = [
            Sample {
                instructions: 10,
                write_entries: 1,
                ..Sample::default()
            },
            Sample {
                instructions: 30,
                write_entries: 3,
                ..Sample::default()
            },
            Sample {
                instructions: 20,
                write_entries: 2,
                ..Sample::default()
            },
        ];
        let stats = HandlerStats::from_samples(&samples).expect("samples");
        assert_eq!(stats.samples, 3);
        assert_eq!(stats.resources.len(), Resource::ALL.len());
        assert_eq!(
            stats.get(Resource::Instructions),
            Some(Stat { max: 30, p50: 20 })
        );
        assert_eq!(
            stats.get(Resource::WriteEntries),
            Some(Stat { max: 3, p50: 2 })
        );
        assert_eq!(stats.get(Resource::MemBytes), Some(Stat { max: 0, p50: 0 }));
        assert_eq!(HandlerStats::from_samples(&[]), None);
    }
}
