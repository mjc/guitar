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
    assert_eq!(oids.len(), 1);
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
fn aliases_keep_distinct_oids_with_shared_prefix_bytes() {
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
fn aliases_lookup_across_many_inserted_oids() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(1, 10);
    let boundary = oid_with_prefix(2, 20);

    let first_alias = oids.get_alias_by_oid(first);
    for suffix in 1..2048 {
        oids.get_alias_by_oid(oid_with_prefix((suffix + 10) as u64, suffix as u64));
    }
    let boundary_alias = oids.get_alias_by_oid(boundary);

    assert_eq!(first_alias, 0);
    assert_eq!(boundary_alias, 2048);
    assert_eq!(oids.get_git2_oid_by_alias(first_alias), first);
    assert_eq!(oids.get_git2_oid_by_alias(boundary_alias), boundary);
    assert_eq!(oids.get_existing_alias(first), Some(first_alias));
    assert_eq!(oids.get_existing_alias(boundary), Some(boundary_alias));
}

#[test]
fn full_oid_lookup_handles_many_similar_oids() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(0xfeed_beef_0000_0001, 10);
    let boundary = oid_with_prefix(0xfeed_beef_ffff_ffff, 20);

    let first_alias = oids.get_alias_by_oid(first);
    for suffix in 1..2048 {
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
fn prefix_lookup_returns_existing_alias_without_reinterning_oid() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(0x1234_5678_0000_0001, 10);
    let second = oid_with_prefix(0xabcd_5678_0000_0001, 20);

    let first_alias = oids.get_alias_by_oid(first);
    let second_alias = oids.get_alias_by_oid(second);

    assert_eq!(oids.get_alias_by_prefix("12345678"), Some(first_alias));
    assert_eq!(oids.get_alias_by_prefix("abcd5678"), Some(second_alias));
    assert_eq!(oids.get_alias_by_prefix("ABCD5678"), Some(second_alias));
    assert_eq!(oids.get_alias_by_prefix("ffff"), None);
    assert_eq!(oids.get_alias_by_prefix("abcdx"), None);
    assert_eq!(oids.len(), 2);
}

#[test]
fn reserve_aliases_preallocates_sorted_aliases() {
    let mut oids = Oids::default();

    oids.reserve_aliases(256);

    assert!(oids.sorted_aliases.capacity() >= 257);
    assert!(oids.alias_oids.capacity() >= 256);
}

#[test]
fn reserve_total_aliases_preallocates_dense_alias_storage_only() {
    let mut oids = Oids::default();

    oids.reserve_total_aliases(256);

    assert!(oids.sorted_aliases.capacity() >= 257);
    assert!(oids.alias_oids.capacity() >= 256);
    assert_eq!(oids.capacity(), 0);
}

#[test]
fn reserve_total_aliases_does_not_preallocate_hash_storage_for_large_hints() {
    let mut oids = Oids::default();

    oids.reserve_total_aliases(100_000);

    assert!(oids.sorted_aliases.capacity() >= 100_001);
    assert_eq!(oids.capacity(), 0);
}
