//! The measurements the host reports for one top-level invocation.

/// One resource the host meters per invocation.
///
/// The twelve variants are the fields of `InvocationResources`, which
/// `env.cost_estimate().resources()` returns. They are the same names as in
/// the snapshot file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resource {
    /// Modelled CPU instructions.
    Instructions,
    /// Peak modelled memory in bytes.
    MemBytes,
    /// Entries read from disk, which includes restored archived entries.
    DiskReadEntries,
    /// Live in-memory ledger entries accessed.
    MemoryReadEntries,
    /// Entries written, which includes restored archived entries.
    WriteEntries,
    /// Bytes read from disk.
    DiskReadBytes,
    /// Bytes written.
    WriteBytes,
    /// Total size of the contract events emitted.
    ContractEventsSizeBytes,
    /// Persistent rent bumps, in ledger-bytes.
    PersistentRentLedgerBytes,
    /// Persistent entries whose rent was bumped.
    PersistentEntryRentBumps,
    /// Temporary rent bumps, in ledger-bytes.
    TemporaryRentLedgerBytes,
    /// Temporary entries whose rent was bumped.
    TemporaryEntryRentBumps,
}

impl Resource {
    /// Every resource, in the order the snapshot file lists them.
    pub const ALL: [Resource; 12] = [
        Resource::Instructions,
        Resource::MemBytes,
        Resource::DiskReadEntries,
        Resource::MemoryReadEntries,
        Resource::WriteEntries,
        Resource::DiskReadBytes,
        Resource::WriteBytes,
        Resource::ContractEventsSizeBytes,
        Resource::PersistentRentLedgerBytes,
        Resource::PersistentEntryRentBumps,
        Resource::TemporaryRentLedgerBytes,
        Resource::TemporaryEntryRentBumps,
    ];

    /// The key used in the snapshot file and in reports.
    pub fn name(self) -> &'static str {
        match self {
            Resource::Instructions => "instructions",
            Resource::MemBytes => "mem_bytes",
            Resource::DiskReadEntries => "disk_read_entries",
            Resource::MemoryReadEntries => "memory_read_entries",
            Resource::WriteEntries => "write_entries",
            Resource::DiskReadBytes => "disk_read_bytes",
            Resource::WriteBytes => "write_bytes",
            Resource::ContractEventsSizeBytes => "contract_events_size_bytes",
            Resource::PersistentRentLedgerBytes => "persistent_rent_ledger_bytes",
            Resource::PersistentEntryRentBumps => "persistent_entry_rent_bumps",
            Resource::TemporaryRentLedgerBytes => "temporary_rent_ledger_bytes",
            Resource::TemporaryEntryRentBumps => "temporary_entry_rent_bumps",
        }
    }

    /// The resource with this file key.
    pub fn from_name(name: &str) -> Option<Resource> {
        Resource::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// One reading of every resource, taken right after one invocation.
///
/// All values are widened to `i64`; the host reports some as `u32`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// See [`Resource::Instructions`].
    pub instructions: i64,
    /// See [`Resource::MemBytes`].
    pub mem_bytes: i64,
    /// See [`Resource::DiskReadEntries`].
    pub disk_read_entries: i64,
    /// See [`Resource::MemoryReadEntries`].
    pub memory_read_entries: i64,
    /// See [`Resource::WriteEntries`].
    pub write_entries: i64,
    /// See [`Resource::DiskReadBytes`].
    pub disk_read_bytes: i64,
    /// See [`Resource::WriteBytes`].
    pub write_bytes: i64,
    /// See [`Resource::ContractEventsSizeBytes`].
    pub contract_events_size_bytes: i64,
    /// See [`Resource::PersistentRentLedgerBytes`].
    pub persistent_rent_ledger_bytes: i64,
    /// See [`Resource::PersistentEntryRentBumps`].
    pub persistent_entry_rent_bumps: i64,
    /// See [`Resource::TemporaryRentLedgerBytes`].
    pub temporary_rent_ledger_bytes: i64,
    /// See [`Resource::TemporaryEntryRentBumps`].
    pub temporary_entry_rent_bumps: i64,
}

impl Sample {
    /// The value of `resource` in this sample.
    pub fn get(&self, resource: Resource) -> i64 {
        match resource {
            Resource::Instructions => self.instructions,
            Resource::MemBytes => self.mem_bytes,
            Resource::DiskReadEntries => self.disk_read_entries,
            Resource::MemoryReadEntries => self.memory_read_entries,
            Resource::WriteEntries => self.write_entries,
            Resource::DiskReadBytes => self.disk_read_bytes,
            Resource::WriteBytes => self.write_bytes,
            Resource::ContractEventsSizeBytes => self.contract_events_size_bytes,
            Resource::PersistentRentLedgerBytes => self.persistent_rent_ledger_bytes,
            Resource::PersistentEntryRentBumps => self.persistent_entry_rent_bumps,
            Resource::TemporaryRentLedgerBytes => self.temporary_rent_ledger_bytes,
            Resource::TemporaryEntryRentBumps => self.temporary_entry_rent_bumps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for resource in Resource::ALL {
            assert_eq!(Resource::from_name(resource.name()), Some(resource));
            assert!(seen.insert(resource.name()));
        }
        assert_eq!(Resource::from_name("fee"), None);
    }

    #[test]
    fn get_reads_the_matching_field() {
        let sample = Sample {
            instructions: 1,
            mem_bytes: 2,
            disk_read_entries: 3,
            memory_read_entries: 4,
            write_entries: 5,
            disk_read_bytes: 6,
            write_bytes: 7,
            contract_events_size_bytes: 8,
            persistent_rent_ledger_bytes: 9,
            persistent_entry_rent_bumps: 10,
            temporary_rent_ledger_bytes: 11,
            temporary_entry_rent_bumps: 12,
        };
        let values: Vec<i64> = Resource::ALL.iter().map(|r| sample.get(*r)).collect();
        assert_eq!(values, (1..=12).collect::<Vec<i64>>());
    }
}
