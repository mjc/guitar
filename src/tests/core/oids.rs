use super::*;

fn oid_with_prefix(prefix: u64, suffix: u64) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[..8].copy_from_slice(&prefix.to_be_bytes());
    bytes[8..16].copy_from_slice(&suffix.to_be_bytes());
    bytes[16..20].copy_from_slice(&(suffix as u32).wrapping_mul(2_654_435_761).to_be_bytes());
    Oid::from_bytes(&bytes).unwrap()
}

#[test]
fn aliases_are_stable_for_repeated_oid() {
    let mut oids = Oids::default();
    let oid = oid_with_prefix(1, 10);

    let first = oids.get_alias_by_oid(oid);
    let second = oids.get_alias_by_oid(oid);

    assert_eq!(first, second);
    assert_eq!(oids.get_git2_oid_by_alias(first), oid);
    assert_eq!(oids.oids.len(), 1);
}

#[test]
fn aliases_keep_distinct_oids_with_shared_prefix() {
    let mut oids = Oids::default();
    let first_oid = oid_with_prefix(1, 10);
    let second_oid = oid_with_prefix(1, 20);

    let first = oids.get_alias_by_oid(first_oid);
    let second = oids.get_alias_by_oid(second_oid);

    assert_ne!(first, second);
    assert_eq!(oids.get_alias_by_oid(first_oid), first);
    assert_eq!(oids.get_alias_by_oid(second_oid), second);
    assert_eq!(oids.get_git2_oid_by_alias(first), first_oid);
    assert_eq!(oids.get_git2_oid_by_alias(second), second_oid);
}

#[test]
fn aliases_keep_distinct_oids_with_shared_32_bit_fingerprint() {
    let mut oids = Oids::default();
    let first_oid = oid_with_prefix(0x1234_5678_0000_0001, 10);
    let second_oid = oid_with_prefix(0x1234_5678_ffff_ffff, 20);

    let first = oids.get_alias_by_oid(first_oid);
    let second = oids.get_alias_by_oid(second_oid);

    assert_ne!(first, second);
    assert_eq!(oids.get_alias_by_oid(first_oid), first);
    assert_eq!(oids.get_alias_by_oid(second_oid), second);
    assert_eq!(oids.get_existing_alias(first_oid), Some(first));
    assert_eq!(oids.get_existing_alias(second_oid), Some(second));
}

#[test]
fn aliases_lookup_across_oid_chunk_boundaries() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(1, 10);
    let boundary = oid_with_prefix(2, 20);

    let first_alias = oids.get_alias_by_oid(first);
    for suffix in 1..OID_CHUNK_SIZE {
        oids.get_alias_by_oid(oid_with_prefix((suffix + 10) as u64, suffix as u64));
    }
    let boundary_alias = oids.get_alias_by_oid(boundary);

    assert_eq!(first_alias, 0);
    assert_eq!(boundary_alias, OID_CHUNK_SIZE as u32);
    assert_eq!(oids.get_git2_oid_by_alias(first_alias), first);
    assert_eq!(oids.get_git2_oid_by_alias(boundary_alias), boundary);
    assert_eq!(oids.get_existing_alias(first), Some(first_alias));
    assert_eq!(oids.get_existing_alias(boundary), Some(boundary_alias));
}

#[test]
fn collision_lookup_works_across_oid_chunk_boundaries() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(0xfeed_beef_0000_0001, 10);
    let boundary = oid_with_prefix(0xfeed_beef_ffff_ffff, 20);

    let first_alias = oids.get_alias_by_oid(first);
    for suffix in 1..OID_CHUNK_SIZE {
        oids.get_alias_by_oid(oid_with_prefix((suffix + 10) as u64, suffix as u64));
    }
    let boundary_alias = oids.get_alias_by_oid(boundary);

    assert_eq!(oids.get_existing_alias(first), Some(first_alias));
    assert_eq!(oids.get_existing_alias(boundary), Some(boundary_alias));
    assert_eq!(oids.get_alias_by_oid(first), first_alias);
    assert_eq!(oids.get_alias_by_oid(boundary), boundary_alias);
}

#[test]
fn missing_alias_lookup_returns_none() {
    let mut oids = Oids::default();
    let present = oid_with_prefix(1, 10);
    let missing = oid_with_prefix(1, 20);

    oids.get_alias_by_oid(present);

    assert_eq!(oids.get_existing_alias(missing), None);
}

#[test]
fn compacted_alias_index_preserves_unique_lookup() {
    let mut oids = Oids::default();
    let input: Vec<_> = (1..=32).map(|prefix| oid_with_prefix(prefix, 10)).collect();

    for &oid in &input {
        oids.get_alias_by_oid(oid);
    }

    oids.compact_alias_index();

    for (expected, &oid) in input.iter().enumerate() {
        assert_eq!(oids.get_existing_alias(oid), Some(expected as u32));
        assert_eq!(oids.get_alias_by_oid(oid), expected as u32);
    }
}

#[test]
fn compacted_alias_index_preserves_collision_lookup() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(7, 10);
    let second = oid_with_prefix(7, 20);
    let third = oid_with_prefix(7, 30);

    let first_alias = oids.get_alias_by_oid(first);
    let second_alias = oids.get_alias_by_oid(second);
    let third_alias = oids.get_alias_by_oid(third);

    oids.compact_alias_index();

    assert_eq!(oids.get_existing_alias(first), Some(first_alias));
    assert_eq!(oids.get_existing_alias(second), Some(second_alias));
    assert_eq!(oids.get_existing_alias(third), Some(third_alias));
}

#[test]
fn insertion_after_compaction_rematerializes_hash_index() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(1, 10);
    let second = oid_with_prefix(2, 20);

    let first_alias = oids.get_alias_by_oid(first);
    oids.compact_alias_index();

    let second_alias = oids.get_alias_by_oid(second);

    assert_ne!(first_alias, second_alias);
    assert_eq!(oids.get_existing_alias(first), Some(first_alias));
    assert_eq!(oids.get_existing_alias(second), Some(second_alias));
}

#[test]
fn reserve_aliases_preallocates_sorted_aliases() {
    let mut oids = Oids::default();

    oids.reserve_aliases(256);

    assert!(oids.sorted_aliases.capacity() >= 257);
}

#[test]
fn reserve_total_aliases_preallocates_hash_and_sorted_storage() {
    let mut oids = Oids::default();

    oids.reserve_total_aliases(256);

    assert!(oids.sorted_aliases.capacity() >= 257);
    assert!(oids.oids.capacity() >= 256);
}
