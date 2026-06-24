use divan::{Bencher, black_box};
use gix::ObjectId;
use guitar::core::oids::Oids;

fn main() {
    divan::main();
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
fn oid_alias_insert_colliding_medium(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(10_000usize)).with_inputs(|| colliding_oids(10_000)).bench_local_values(|input| black_box(insert_aliases(&input)));
}

#[divan::bench(sample_count = 50, sample_size = 10)]
fn oid_alias_lookup_existing_colliding_medium(bencher: Bencher) {
    bencher.counter(divan::counter::ItemsCount::new(10_000usize)).with_inputs(|| colliding_oids(10_000)).bench_local_values(|input| black_box(lookup_existing_aliases(&input)));
}
