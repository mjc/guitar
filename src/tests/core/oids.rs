use super::*;

fn oid_with_prefix(prefix: u64, suffix: u64) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[..8].copy_from_slice(&prefix.to_be_bytes());
    bytes[8..16].copy_from_slice(&suffix.to_be_bytes());
    bytes[16..20].copy_from_slice(&(suffix as u32).wrapping_mul(2_654_435_761).to_be_bytes());
    Oid::from_bytes(&bytes).unwrap()
}

fn assert_round_trip(oids: &mut Oids, oid: Oid, alias: u32) {
    assert_eq!(oids.get_alias_by_oid(oid), alias);
    assert_eq!(oids.get_existing_alias(oid), Some(alias));
    assert_eq!(oids.get_git2_oid_by_alias(alias), oid);
}

#[test]
fn aliases_round_trip_distinct_oids_with_shared_prefixes() {
    for (first_prefix, second_prefix) in [(1, 1), (0x1234_5678_0000_0001, 0x1234_5678_ffff_ffff)] {
        let mut oids = Oids::default();
        let first_oid = oid_with_prefix(first_prefix, 10);
        let second_oid = oid_with_prefix(second_prefix, 20);
        let first = oids.get_alias_by_oid(first_oid);
        let second = oids.get_alias_by_oid(second_oid);

        assert_ne!(first, second);
        assert_round_trip(&mut oids, first_oid, first);
        assert_round_trip(&mut oids, second_oid, second);
        assert_eq!(oids.len(), 2);
    }
}

#[test]
fn aliases_lookup_across_many_similar_inserted_oids() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(0xfeed_beef_0000_0001, 10);
    let boundary = oid_with_prefix(0xfeed_beef_ffff_ffff, 20);
    let first_alias = oids.get_alias_by_oid(first);

    for suffix in 1..2048 {
        oids.get_alias_by_oid(oid_with_prefix(0xfeed_beef_0000_0000 | suffix, suffix + 10_000));
    }

    let boundary_alias = oids.get_alias_by_oid(boundary);
    assert_eq!(first_alias, 0);
    assert_eq!(boundary_alias, 2048);
    assert_round_trip(&mut oids, first, first_alias);
    assert_round_trip(&mut oids, boundary, boundary_alias);
}

#[test]
fn prefix_lookup_returns_existing_alias_without_reinterning_oid() {
    let mut oids = Oids::default();
    let first = oid_with_prefix(0x1234_5678_0000_0001, 10);
    let second = oid_with_prefix(0xabcd_5678_0000_0001, 20);
    let missing = oid_with_prefix(0xffff_5678_0000_0001, 30);

    let first_alias = oids.get_alias_by_oid(first);
    let second_alias = oids.get_alias_by_oid(second);

    assert_eq!(oids.get_alias_by_prefix("12345678"), Some(first_alias));
    assert_eq!(oids.get_alias_by_prefix("abcd5678"), Some(second_alias));
    assert_eq!(oids.get_alias_by_prefix("ABCD5678"), Some(second_alias));
    assert_eq!(oids.get_alias_by_prefix("ffff"), None);
    assert_eq!(oids.get_alias_by_prefix("abcdx"), None);
    assert_eq!(oids.get_existing_alias(missing), None);
    assert_eq!(oids.len(), 2);
}

#[derive(Clone, Copy)]
enum Reserve {
    Additional,
    Total,
}

#[test]
fn reserve_preallocates_dense_alias_storage_without_hash_storage() {
    for (reserve, aliases) in [(Reserve::Additional, 256), (Reserve::Total, 256), (Reserve::Total, 100_000)] {
        let mut oids = Oids::default();

        match reserve {
            Reserve::Additional => oids.reserve_aliases(aliases),
            Reserve::Total => oids.reserve_total_aliases(aliases),
        }

        assert!(oids.sorted_aliases.capacity() > aliases);
        assert!(oids.alias_oids.capacity() >= aliases);
        if matches!(reserve, Reserve::Total) {
            assert_eq!(oids.capacity(), 0);
        }
    }
}
