use super::*;

#[test]
fn ratio_of_live_does_not_overflow_or_lose_remainders() {
    assert_eq!(Ratio::new(1, 2).of(7), 3);
    assert_eq!(Ratio::new(3, 2).of(7), 10);
    assert_eq!(Ratio::new(8, 1).of(usize::MAX), usize::MAX);
}

#[test]
fn policies_name_their_tables_and_page_release() {
    for (index, policy) in MemoryPolicy::ALL.into_iter().enumerate() {
        assert_eq!(policy as usize, index);
        assert_eq!(MemoryPolicy::from_name(policy.name()), Some(policy));
    }
    assert_eq!(MemoryPolicy::default(), MemoryPolicy::Throughput);
    assert_eq!(MemoryPolicy::from_name("speed"), None);
    assert!(MemoryPolicy::MemoryFirst.returns_pages_at_collections());
    assert!(!MemoryPolicy::Throughput.returns_pages_at_collections());
    // Throughput collects no more often than memory-first at any live size.
    let (throughput, memory_first) = (THROUGHPUT_PACING, MEMORY_FIRST_PACING);
    for live in [0, 1 << 10, 1 << 16, 1 << 20, 1 << 24] {
        assert!(throughput.young_budget(live) >= memory_first.young_budget(live));
    }
    assert!(throughput.minimum_allocation_cap > memory_first.minimum_allocation_cap);
    // Survival widens a steady heap's young budget past a quarter only.
    let below = SURVIVING_YOUNG_FROM - 1;
    assert_eq!(throughput.surviving_young_budget(0, below, true), throughput.young_budget(0));
    assert_eq!(
        throughput.surviving_young_budget(0, SURVIVING_YOUNG_FROM, true),
        throughput.young_budget(0) * THROUGHPUT_SURVIVING_YOUNG_FACTOR
    );
    // A growing heap keeps its budget whatever survives.
    assert_eq!(
        throughput.surviving_young_budget(0, SURVIVAL_SCALE, false),
        throughput.young_budget(0)
    );
    assert_eq!(
        memory_first.surviving_young_budget(0, SURVIVAL_SCALE, true),
        memory_first.young_budget(0) * MEMORY_FIRST_SURVIVING_YOUNG_FACTOR
    );
    assert_eq!(
        memory_first.surviving_young_budget(0, SURVIVAL_SCALE, false),
        memory_first.young_budget(0)
    );
    assert!(throughput.maximum_cap_backoff > memory_first.maximum_cap_backoff);
}

#[test]
fn full_collection_follows_old_growth_or_the_allocation_cap() {
    for policy in MemoryPolicy::ALL {
        full_collection_follows_old_growth_or_the_allocation_cap_under(policy.table());
    }
}

fn full_collection_follows_old_growth_or_the_allocation_cap_under(pacing: GenerationalPacing) {
    let live = 1 << 20;
    // The growth and the allocation that make a full collection due, each at least the
    // table's minimum interval.
    let growth = pacing.old_growth.of(live).max(pacing.minimum_old_growth);
    let cap = pacing.allocation_cap.of(live).max(pacing.minimum_allocation_cap);
    assert!(!pacing.full_due(live, live + growth - 1, 0, 0));
    assert!(pacing.full_due(live, live + growth, 0, 0));
    assert!(!pacing.full_due(live, live, cap - 1, 0));
    assert!(pacing.full_due(live, live, cap, 0));
    assert_eq!(pacing.young_budget(0), pacing.minimum_young);
    // A live set whose young share lies between the minimum and the maximum budget, in a
    // table whose budget may grow with the live set.
    if pacing.minimum_young < pacing.maximum_young {
        let between = pacing.minimum_young * 2 * pacing.young.denominator / pacing.young.numerator;
        assert_eq!(pacing.young_budget(between), pacing.young.of(between));
    }
    let huge = pacing.maximum_young * 1024;
    assert_eq!(pacing.young_budget(huge), pacing.maximum_young);
}

/// Throughput's table with aging turned on only by full collections' evidence, as it is
/// once its first aged survivors outlive their second minor.
fn aging_on_evidence() -> GenerationalPacing {
    GenerationalPacing { ages_from_start: false, ..MemoryPolicy::Throughput.table() }
}

#[test]
fn throughput_ages_from_its_first_collection_until_aged_survivors_outlive_their_second_minor() {
    let pacing = MemoryPolicy::Throughput.table();
    let live = 1 << 20;
    let promoted = 1 << 16;
    let mut state = PacingState::default();
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    pacing.plan(&mut state, minor(live + promoted, live, promoted));
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    let freed = AgedFates { died: 9, promoted: 1 };
    pacing.plan(&mut state, CollectionOutcome { aged: freed, ..minor(live, live, promoted) });
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    let mut state = PacingState::default();
    pacing.plan(&mut state, minor(live + promoted, live, promoted));
    let outliving = AgedFates { died: 1, promoted: 9 };
    pacing.plan(&mut state, CollectionOutcome { aged: outliving, ..minor(live, live, promoted) });
    assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
    let memory_first = MemoryPolicy::MemoryFirst.table();
    assert_eq!(PacingState::default().promotion_age(&memory_first), memory_first.promotion_age);
}

fn minor(live: usize, previous_live: usize, allocated: usize) -> CollectionOutcome {
    CollectionOutcome {
        full: false,
        live,
        live_weight: live,
        previous_live_weight: previous_live,
        live_after_full: previous_live,
        live_weight_after_full: previous_live,
        allocated,
        since_full: allocated,
        aged: AgedFates::default(),
    }
}

/// A full collection after minors promoted `promoted` weight onto `live`, which keeps
/// `kept` of it.
fn full_after_promoting(live: usize, promoted: usize, kept: usize) -> CollectionOutcome {
    CollectionOutcome { full: true, since_full: 0, ..minor(live + kept, live + promoted, 0) }
}

/// Minors promote `promoted` onto `live`, and the full collection after them finds every
/// promotion dead.
fn promotions_die_old(pacing: &GenerationalPacing, state: &mut PacingState, live: usize, promoted: usize) {
    pacing.plan(state, minor(live + promoted, live, promoted));
    pacing.plan(state, full_after_promoting(live, promoted, 0));
}

#[test]
fn promotions_dying_old_turn_aging_on_until_aged_survivors_outlive_their_second_minor() {
    let pacing = aging_on_evidence();
    let mut state = PacingState::default();
    let live = 1 << 20;
    let promoted = 1 << 16;
    turn_aging_on(&pacing, &mut state, live, promoted);
    // Aged survivors that their second minor frees keep aging on.
    let freed = AgedFates { died: 9, promoted: 1 };
    pacing.plan(&mut state, CollectionOutcome { aged: freed, ..minor(live, live, promoted) });
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    // Full collections that find little promoted garbage, because aging freed it young,
    // leave it on: the server case.
    for _ in 0..=pacing.aging_interval {
        pacing.plan(&mut state, full_after_promoting(live, promoted, promoted));
        pacing.plan(&mut state, CollectionOutcome { aged: freed, ..minor(live, live, promoted) });
        assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    }
    // Aged survivors that outlive it turn aging off, for the aging interval of fulls.
    let outliving = AgedFates { died: 1, promoted: 9 };
    for _ in 0..2 {
        pacing.plan(&mut state, CollectionOutcome { aged: outliving, ..minor(live, live, promoted) });
    }
    assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
    for _ in 0..pacing.aging_interval + FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE - 1 {
        promotions_die_old(&pacing, &mut state, live, promoted);
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
    }
    promotions_die_old(&pacing, &mut state, live, promoted);
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
}

/// Turns aging on from the table's age: one full collection finding promotions dead is not
/// enough; one that finds them living on in between starts the count again.
fn turn_aging_on(pacing: &GenerationalPacing, state: &mut PacingState, live: usize, promoted: usize) {
    assert_eq!(state.promotion_age(pacing), pacing.promotion_age);
    promotions_die_old(pacing, state, live, promoted);
    assert_eq!(state.promotion_age(pacing), pacing.promotion_age);
    pacing.plan(state, full_after_promoting(live, promoted, promoted));
    for _ in 1..FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
        promotions_die_old(pacing, state, live, promoted);
        assert_eq!(state.promotion_age(pacing), pacing.promotion_age);
    }
    promotions_die_old(pacing, state, live, promoted);
    assert_eq!(state.promotion_age(pacing), PromotionAge::OLDEST);
}

#[test]
fn aging_that_outlives_its_first_measurement_turns_off_at_once() {
    let pacing = aging_on_evidence();
    let mut state = PacingState::default();
    let live = 1 << 20;
    let promoted = 1 << 16;
    for _ in 0..FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
        promotions_die_old(&pacing, &mut state, live, promoted);
    }
    assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    let outliving = AgedFates { died: 1, promoted: 9 };
    pacing.plan(&mut state, CollectionOutcome { aged: outliving, ..minor(live, live, promoted) });
    assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
}

#[test]
fn promotions_that_live_on_keep_the_first_age_and_memory_first_never_ages() {
    let live = 1 << 20;
    let promoted = 1 << 16;
    let pacing = aging_on_evidence();
    let mut state = PacingState::default();
    pacing.plan(&mut state, minor(live + promoted, live, promoted));
    pacing.plan(&mut state, full_after_promoting(live, promoted, promoted));
    assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
    let pacing = MemoryPolicy::MemoryFirst.table();
    let mut state = PacingState::default();
    pacing.plan(&mut state, minor(live + promoted, live, promoted));
    pacing.plan(&mut state, full_after_promoting(live, promoted, 0));
    assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
}

#[test]
fn unproductive_full_collections_back_off_the_allocation_cap_up_to_the_table_limit() {
    for policy in MemoryPolicy::ALL {
        let pacing = policy.table();
        let mut state = PacingState::default();
        let live = 1 << 20;
        // Full collections that keep everything they were expected to keep reclaim nothing.
        let unproductive = CollectionOutcome { full: true, since_full: 0, ..minor(live, live, 0) };
        for _ in 0..=pacing.maximum_cap_backoff {
            pacing.plan(&mut state, unproductive);
        }
        assert_eq!(state.cap_backoff, pacing.maximum_cap_backoff);
        let cap = pacing.allocation_cap.of(live).max(pacing.minimum_allocation_cap);
        let backed_off = cap << pacing.maximum_cap_backoff;
        assert!(!pacing.allocation_cap_reached(live, backed_off - 1, state.cap_backoff));
        assert!(pacing.allocation_cap_reached(live, backed_off, state.cap_backoff));
        // A productive one restores the cap.
        let productive = CollectionOutcome { live_weight: 0, ..unproductive };
        pacing.plan(&mut state, productive);
        assert_eq!(state.cap_backoff, 0);
    }
}

#[test]
fn high_young_survival_switches_to_full_collections_and_probes_back() {
    for policy in MemoryPolicy::ALL {
        high_young_survival_switches_to_full_collections_and_probes_back_under(policy.table());
    }
}

fn high_young_survival_switches_to_full_collections_and_probes_back_under(pacing: GenerationalPacing) {
    let mut state = PacingState::default();
    let live = 1 << 20;
    // Young allocations die: minors.
    let next = pacing.plan(&mut state, minor(live, live, live / 2));
    assert_eq!(state.regime(&pacing), Regime::Generational);
    assert!(!next.full);
    // Nearly everything young survives until smoothed survival passes the table's limit:
    // full collections with full-only headroom.
    while (state.survival + SURVIVAL_SCALE) / 2 <= pacing.survival_limit {
        pacing.plan(&mut state, minor(live + live / 2, live, live / 2));
    }
    let next = pacing.plan(&mut state, minor(2 * live, 3 * live / 2, live / 2));
    assert_eq!(state.regime(&pacing), Regime::FullOnly);
    // The next full collection keeps the interval measured from the last full one.
    let interval = pacing.full_only_headroom.of(3 * live / 2) - live / 2;
    assert_eq!(next, NextCollection { threshold: interval.max(pacing.minimum_young), full: true });
    // After `probe_interval` full collections, a minor probes survival again.
    let full = CollectionOutcome { full: true, since_full: 0, ..minor(live, live, 4 * live) };
    for _ in 1..pacing.probe_interval {
        assert!(pacing.plan(&mut state, full).full);
    }
    assert!(!pacing.plan(&mut state, full).full);
    // The probe finds young allocations dying again: back to minors.
    pacing.plan(&mut state, minor(live, live, live / 2));
    pacing.plan(&mut state, minor(live, live, live / 2));
    assert_eq!(state.regime(&pacing), Regime::Generational);
}
