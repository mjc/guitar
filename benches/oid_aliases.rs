use divan::{Bencher, black_box};
use gix::ObjectId;
use guitar::core::oids::Oids;
use iddqd::{BiHashItem, BiHashMap, IdHashItem, IdHashMap, bi_upcast, id_upcast};

fn main() {
    divan::main();
}

#[derive(Debug)]
struct IddqdAlias {
    oid: ObjectId,
    alias: u32,
}

impl IdHashItem for IddqdAlias {
    type Key<'a> = &'a ObjectId;

    fn key(&self) -> Self::Key<'_> {
        &self.oid
    }

    id_upcast!();
}

#[derive(Default)]
struct IddqdOidAliases {
    by_oid: IdHashMap<IddqdAlias>,
    oids: Vec<ObjectId>,
}

impl IddqdOidAliases {
    fn reserve_aliases(&mut self, additional: usize) {
        self.by_oid.reserve(additional);
        self.oids.reserve(additional);
    }

    fn get_alias_by_oid(&mut self, oid: ObjectId) -> u32 {
        if let Some(existing) = self.by_oid.get(&oid) {
            return existing.alias;
        }

        let alias = self.oids.len() as u32;
        self.oids.push(oid);
        self.by_oid.insert_unique(IddqdAlias { oid, alias }).unwrap();
        alias
    }
}

#[derive(Debug)]
struct BiIddqdAlias {
    alias: u32,
    oid: ObjectId,
}

impl BiHashItem for BiIddqdAlias {
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

#[derive(Default)]
struct BiIddqdOidAliases {
    aliases: BiHashMap<BiIddqdAlias>,
    next_alias: u32,
}

impl BiIddqdOidAliases {
    fn reserve_aliases(&mut self, additional: usize) {
        self.aliases.reserve(additional);
    }

    fn get_alias_by_oid(&mut self, oid: ObjectId) -> u32 {
        if let Some(existing) = self.aliases.get2(&oid) {
            return existing.alias;
        }

        let alias = self.next_alias;
        self.next_alias += 1;
        self.aliases.insert_unique(BiIddqdAlias { alias, oid }).unwrap();
        alias
    }

    fn get_oid_by_alias(&self, alias: u32) -> Option<&ObjectId> {
        self.aliases.get1(&alias).map(|record| &record.oid)
    }
}

fn oid_for(index: usize) -> ObjectId {
    let mut bytes = [0u8; 20];
    let high = ((index as u64) << 32) | (index.wrapping_mul(2_654_435_761) as u32 as u64);
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..16].copy_from_slice(&(index.wrapping_mul(1_000_003) as u64).to_be_bytes());
    bytes[16..20].copy_from_slice(&(index as u32).wrapping_mul(2_654_435_761).to_be_bytes());
    ObjectId::from_bytes_or_panic(&bytes)
}

fn colliding_oid_for(index: usize) -> ObjectId {
    let mut bytes = [0u8; 20];
    bytes[..8].copy_from_slice(&1u64.to_be_bytes());
    bytes[8..16].copy_from_slice(&(index as u64).to_be_bytes());
    bytes[16..20].copy_from_slice(&(index as u32).wrapping_mul(2_654_435_761).to_be_bytes());
    ObjectId::from_bytes_or_panic(&bytes)
}

fn oids(count: usize) -> Vec<ObjectId> {
    (0..count).map(oid_for).collect()
}

fn colliding_oids(count: usize) -> Vec<ObjectId> {
    (0..count).map(colliding_oid_for).collect()
}

fn insert_aliases(input: &[ObjectId]) -> usize {
    let mut aliases = Oids::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.len())
}

fn insert_aliases_without_reserve(input: &[ObjectId]) -> usize {
    let mut aliases = Oids::default();

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.len())
}

fn insert_aliases_with_total_hint(input: &[ObjectId]) -> usize {
    let mut aliases = Oids::default();
    aliases.reserve_total_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.len())
}

fn insert_bi_iddqd_aliases_with_full_reserve(input: &[ObjectId]) -> usize {
    let mut aliases = BiIddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.next_alias as usize)
}

fn lookup_existing_aliases(input: &[ObjectId]) -> u32 {
    let mut aliases = Oids::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    input.iter().fold(0, |acc, &oid| acc ^ aliases.get_alias_by_oid(oid))
}

fn insert_aliases_in_batches(input: &[ObjectId], batch_size: usize) -> usize {
    let mut aliases = Oids::default();

    for batch in input.chunks(batch_size) {
        aliases.reserve_aliases(batch.len());
        for &oid in batch {
            aliases.get_alias_by_oid(oid);
        }
    }

    black_box(aliases.len())
}

fn insert_iddqd_aliases(input: &[ObjectId]) -> usize {
    let mut aliases = IddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.oids.len())
}

fn lookup_existing_iddqd_aliases(input: &[ObjectId]) -> u32 {
    let mut aliases = IddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    input.iter().fold(0, |acc, &oid| acc ^ aliases.get_alias_by_oid(oid))
}

fn insert_bi_iddqd_aliases(input: &[ObjectId]) -> usize {
    let mut aliases = BiIddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    black_box(aliases.next_alias as usize)
}

fn lookup_existing_bi_iddqd_aliases(input: &[ObjectId]) -> u32 {
    let mut aliases = BiIddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    input.iter().fold(0, |acc, &oid| acc ^ aliases.get_alias_by_oid(oid))
}

fn lookup_bi_iddqd_alias_to_oid(input: &[ObjectId]) -> u8 {
    let mut aliases = BiIddqdOidAliases::default();
    aliases.reserve_aliases(input.len());

    for &oid in input {
        aliases.get_alias_by_oid(oid);
    }

    (0..input.len() as u32).fold(0, |acc, alias| acc ^ aliases.get_oid_by_alias(alias).map_or(0, |oid| oid.as_bytes()[0]))
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_medium(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(10_000usize)).with_inputs(|| oids(10_000)).bench_local_values(|input| black_box(insert_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_large_batched(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_aliases_in_batches(&input, 2_000)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_large_no_reserve(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_aliases_without_reserve(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_large_total_hint(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_aliases_with_total_hint(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_existing_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(lookup_existing_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_iddqd_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_iddqd_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_existing_iddqd_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(lookup_existing_iddqd_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_bi_iddqd_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_bi_iddqd_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_bi_iddqd_large_full_reserve(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(insert_bi_iddqd_aliases_with_full_reserve(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_existing_bi_iddqd_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(lookup_existing_bi_iddqd_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_bi_iddqd_alias_to_oid_large(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(100_000usize)).with_inputs(|| oids(100_000)).bench_local_values(|input| black_box(lookup_bi_iddqd_alias_to_oid(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_insert_colliding_medium(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(10_000usize)).with_inputs(|| colliding_oids(10_000)).bench_local_values(|input| black_box(insert_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_existing_colliding_medium(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(10_000usize)).with_inputs(|| colliding_oids(10_000)).bench_local_values(|input| black_box(lookup_existing_aliases(&input)));
}
