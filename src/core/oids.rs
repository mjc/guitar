use crate::core::chunk::NONE;
use git2::Oid;
use gix::ObjectId;
use iddqd::{BiHashItem, BiHashMap, bi_upcast};

pub trait IntoGixOid {
    fn into_gix_oid(self) -> ObjectId;
}

impl IntoGixOid for ObjectId {
    fn into_gix_oid(self) -> ObjectId {
        self
    }
}

impl IntoGixOid for Oid {
    fn into_gix_oid(self) -> ObjectId {
        git2_to_gix_oid(self)
    }
}

pub fn git2_to_gix_oid(oid: Oid) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(oid.as_bytes())
}

#[cfg(test)]
#[path = "../tests/core/oids.rs"]
mod tests;

pub fn gix_to_git2_oid(oid: gix::ObjectId) -> Oid {
    Oid::from_bytes(oid.as_bytes()).unwrap()
}

// Stores full OIDs once and passes small numeric aliases through UI data structures.
#[derive(Clone)]
pub struct Oids {
    pub zero: ObjectId,
    records: BiHashMap<OidRecord>,
    next_alias: u32,
    pub sorted_aliases: Vec<u32>,
    pub stashes: Vec<u32>,
}

#[derive(Clone, Debug)]
struct OidRecord {
    alias: u32,
    oid: ObjectId,
}

impl BiHashItem for OidRecord {
    type K1<'a> = u32;
    type K2<'a> = &'a ObjectId;

    fn key1(&self) -> Self::K1<'_> {
        self.alias
    }

    fn key2(&self) -> Self::K2<'_> {
        &self.oid
    }

    bi_upcast!();
}

impl Default for Oids {
    fn default() -> Self {
        Oids { zero: ObjectId::null(gix::hash::Kind::Sha1), records: BiHashMap::default(), next_alias: 0, sorted_aliases: vec![NONE], stashes: vec![] }
    }
}

impl Oids {
    pub fn reserve_total_aliases(&mut self, total: usize) {
        let sorted_target = total.saturating_add(1);
        let sorted_spare = self.sorted_aliases.capacity().saturating_sub(self.sorted_aliases.len());
        if sorted_target > self.sorted_aliases.len() + sorted_spare {
            self.sorted_aliases.reserve(sorted_target - self.sorted_aliases.len() - sorted_spare);
        }
    }

    pub fn reserve_aliases(&mut self, additional: usize) {
        let sorted_spare = self.sorted_aliases.capacity().saturating_sub(self.sorted_aliases.len());
        if additional > sorted_spare {
            self.sorted_aliases.reserve(additional - sorted_spare);
        }
    }

    pub fn compact_alias_index(&mut self) {
        self.records.shrink_to_fit();
    }

    pub fn shrink_to_fit(&mut self) {
        self.records.shrink_to_fit();
        self.sorted_aliases.shrink_to_fit();
        self.stashes.shrink_to_fit();
    }

    pub fn get_alias_by_oid(&mut self, oid: impl IntoGixOid) -> u32 {
        let oid = oid.into_gix_oid();
        if let Some(record) = self.records.get2(&oid) {
            return record.alias;
        }

        let alias = self.next_alias;
        self.next_alias = self.next_alias.checked_add(1).expect("OID alias space exhausted");
        self.records.insert_unique(OidRecord { alias, oid }).expect("new OID record has unique alias and OID");

        alias
    }

    pub fn get_existing_alias(&self, oid: impl IntoGixOid) -> Option<u32> {
        let oid = oid.into_gix_oid();
        self.records.get2(&oid).map(|record| record.alias)
    }

    pub fn get_alias_by_idx(&self, idx: usize) -> u32 {
        *self.sorted_aliases.get(idx).unwrap()
    }

    pub fn get_oid_by_alias(&self, alias: u32) -> &ObjectId {
        self.records.get1(&alias).map_or(&self.zero, |record| &record.oid)
    }

    pub fn get_git2_oid_by_alias(&self, alias: u32) -> Oid {
        gix_to_git2_oid(*self.get_oid_by_alias(alias))
    }

    pub fn get_sorted_aliases(&self) -> &Vec<u32> {
        &self.sorted_aliases
    }

    pub fn append_sorted_alias(&mut self, alias: u32) {
        self.sorted_aliases.push(alias);
    }

    pub fn get_commit_count(&self) -> usize {
        self.sorted_aliases.len()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn capacity(&self) -> usize {
        self.records.capacity()
    }

    pub fn iter_oids(&self) -> impl Iterator<Item = &ObjectId> {
        self.records.iter().map(|record| &record.oid)
    }

    pub fn get_alias_by_prefix(&self, prefix: &str) -> Option<u32> {
        self.records.iter().find(|record| oid_starts_with_hex_prefix(&record.oid, prefix)).map(|record| record.alias)
    }

    pub fn is_zero(&self, oid: &ObjectId) -> bool {
        self.zero == *oid
    }
}

fn oid_starts_with_hex_prefix(oid: &ObjectId, prefix: &str) -> bool {
    prefix.bytes().enumerate().all(|(idx, byte)| {
        let Some(nibble) = hex_nibble(byte) else {
            return false;
        };
        let oid_byte = oid.as_bytes()[idx / 2];
        let oid_nibble = if idx % 2 == 0 { oid_byte >> 4 } else { oid_byte & 0x0f };
        nibble == oid_nibble
    })
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
