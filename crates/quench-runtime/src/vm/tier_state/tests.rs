use super::*;
use crate::vm::compiled::tier_policy::TIERS;

/// A controller of numbers: bodies are `u32`s, feedback and stats are bitsets of the places left.
struct Probe;

/// Entries before a body compiles, whatever the history.
const WARMUP: u32 = 3;
/// Leaves a body keeps before it is judged.
const LEAVES: u32 = 2;
/// The entries an interpreted function runs before it is revived.
const REVIVE: u32 = 5;

#[derive(Default)]
struct Left {
    leaves: u32,
    places: u8,
}

impl Controller for Probe {
    type Pending = u32;
    type Artifact = u32;
    type Feedback = u8;
    type Stats = Left;
    type Leave = u8;

    fn warmup(_: &Tiers, _: &Past<u8>) -> u32 {
        WARMUP
    }

    fn judge(_: &Tiers, stats: &mut Left, past: &Past<u8>, at: u8) -> Verdict<u8> {
        stats.leaves += 1;
        stats.places |= 1 << at;
        if stats.leaves < LEAVES {
            return Verdict::Keep;
        }
        Verdict::Respecialize(past.feedback | stats.places)
    }

    fn completed(stats: &mut Left) {
        stats.leaves = 0;
    }

    fn learned(stats: &Left, feedback: &u8) -> u8 {
        feedback | stats.places
    }

    fn revives(_: &Tiers, entries: u32) -> bool {
        entries >= REVIVE
    }
}

type T = Tier<Probe>;

fn enter(tier: T) -> (T, Why) {
    tier.transition(Event::Entry { favourable: true }, &TIERS)
}

fn warm(mut tier: T) -> T {
    for _ in 0..WARMUP {
        tier = enter(tier).0;
    }
    tier
}

fn publish(tier: T, artifact: u32) -> T {
    tier.transition(Event::Compiled(artifact), &TIERS).0
}

fn leave(tier: T, at: u8) -> (T, Why) {
    tier.transition(Event::GuardLeft { at }, &TIERS)
}

/// Leaves a speculative body until it is replaced, and the successor it asks for.
fn exhaust(mut tier: T) -> T {
    loop {
        let (next, why) = leave(tier, 0);
        tier = next;
        if why != Why::Counted {
            return tier;
        }
    }
}

#[test]
fn a_cold_tier_warms_and_queues_when_the_warmup_ends() {
    let (tier, why) = enter(T::Cold);
    assert!(matches!(tier, Tier::Warming { entries: 1, .. }) && why == Why::Counted);
    let tier = warm(T::Cold);
    assert!(matches!(tier, Tier::Queued { .. }));
    let (tier, why) = enter(tier);
    assert!(matches!(tier, Tier::Queued { .. }) && why == Why::Counted, "a queued tier waits for its owner");
}

#[test]
fn counting_is_the_warming_fast_path_and_leaves_the_boundary_to_the_transition() {
    let mut tier = enter(T::Cold).0;
    assert!(tier.count(true, &TIERS));
    assert!(!tier.count(true, &TIERS), "the entry that ends the warm-up is the transition's");
    assert!(matches!(tier, Tier::Warming { entries: 2, favourable: 2, .. }), "and is not counted twice");
    let mut cold = T::Cold;
    assert!(cold.count(true, &TIERS), "the first entry starts the warm-up");
    assert!(matches!(cold, Tier::Warming { entries: 1, favourable: 1, .. }));
    assert!(!T::Queued { past: Past::default() }.count(true, &TIERS));
}

/// A look at `build` where the code generator's answer is `answer`.
fn look(build: Build<u32>, interval: u16, answer: fn(Box<u32>) -> Polled<u32, u32>) -> Looked<u32, u32> {
    build.poll(interval, answer)
}

fn unreachable_look(_: Box<u32>) -> Polled<u32, u32> {
    unreachable!("the countdown has not ended")
}

#[test]
fn a_build_counts_down_before_each_look_and_waits_the_interval_again_where_not_ready() {
    let mut build = Build::new(Box::new(7), 1);
    for answer in [unreachable_look as fn(_) -> _, Polled::Pending] {
        let Looked::Pending(next) = look(build, 2, answer) else { panic!("pending") };
        build = next;
    }
    for _ in 0..2 {
        let Looked::Pending(next) = look(build, 2, unreachable_look) else { panic!("waits the interval") };
        build = next;
    }
    assert_eq!(*build.pending(), 7);
    assert!(matches!(look(build, 2, |pending| Polled::Ready(*pending + 1)), Looked::Ready(8)));
    assert!(matches!(look(Build::new(Box::new(0), 0), 2, |_| Polled::Stale), Looked::Stale));
    assert!(matches!(look(Build::new(Box::new(0), 0), 2, |_| Polled::Failed), Looked::Failed));
}

#[test]
fn a_compile_starts_from_the_queue_and_publishes_a_speculative_body() {
    let (tier, why) = warm(T::Cold).transition(Event::Started(Build::new(Box::new(7), 0)), &TIERS);
    assert!(tier.is_compiling() && why == Why::Started);
    let (tier, why) = tier.transition(Event::Compiled(8), &TIERS);
    assert!(matches!(tier, Tier::Speculative { artifact: 8, past: Past { bodies: 1, .. }, .. }) && why == Why::Published);
}

#[test]
fn a_body_that_leaves_too_often_is_replaced_without_what_it_left_at() {
    let tier = publish(warm(T::Cold), 1);
    let (tier, why) = leave(tier, 3);
    assert!(matches!(tier, Tier::Speculative { .. }) && why == Why::Counted);
    let (tier, why) = leave(tier, 5);
    let Tier::Warming { entries: 0, past, .. } = tier else { panic!("warms again") };
    assert_eq!((why, past.bodies, past.feedback), (Why::LeftTooOften, 1, 1 << 3 | 1 << 5));
}

#[test]
fn a_completed_run_pays_the_leaves_back() {
    let tier = publish(warm(T::Cold), 1);
    let (tier, _) = leave(tier, 0);
    let tier = tier.transition(Event::COMPLETED, &TIERS).0;
    let (tier, why) = leave(tier, 0);
    assert!(matches!(tier, Tier::Speculative { .. }) && why == Why::Counted);
}

#[test]
fn after_the_speculative_bodies_the_next_is_the_one_permanent_generic_body() {
    let mut tier = T::Cold;
    for body in 1..=SPECULATIVE_BODIES {
        tier = publish(warm(tier), u32::from(body));
        assert!(matches!(tier, Tier::Speculative { past: Past { bodies, .. }, .. } if bodies == body));
        tier = exhaust(tier);
    }
    tier = publish(warm(tier), 99);
    assert!(matches!(tier, Tier::Generic { artifact: 99 }));
    let (tier, why) = leave(tier, 0);
    assert!(matches!(tier, Tier::Generic { .. }) && why == Why::Counted, "a generic body assumes nothing to leave");
    let tier = tier.transition(Event::COMPLETED, &TIERS).0;
    assert!(matches!(tier, Tier::Generic { artifact: 99 }));
}

#[test]
fn generalizing_skips_to_the_generic_body_from_any_state_before_it() {
    let (tier, why) = T::Cold.transition(Event::GENERALIZE, &TIERS);
    assert!(matches!(&tier, Tier::Warming { past, .. } if past.is_spent()) && why == Why::Generalized);
    assert!(matches!(publish(warm(tier), 4), Tier::Generic { artifact: 4 }));
    let speculative = publish(warm(T::Cold), 1);
    let (tier, why) = speculative.transition(Event::GENERALIZE, &TIERS);
    assert!(matches!(&tier, Tier::Warming { past, .. } if past.is_spent()) && why == Why::Generalized);
}

#[test]
fn a_retired_body_warms_again_and_a_retired_generic_body_stays_generic() {
    let tier = leave(publish(warm(T::Cold), 1), 2).0;
    let (tier, why) = tier.transition(Event::INVALIDATED, &TIERS);
    let Tier::Warming { past, .. } = &tier else { panic!("warms") };
    assert_eq!((why, past.bodies, past.feedback), (Why::Invalidated, 1, 1 << 2), "its leaves are its successor's feedback");
    let generic = publish(warm(T::Cold.transition(Event::GENERALIZE, &TIERS).0), 5);
    let (tier, why) = generic.transition(Event::INVALIDATED, &TIERS);
    assert!(matches!(&tier, Tier::Warming { past, .. } if past.is_spent()) && why == Why::Invalidated);
    assert!(matches!(publish(warm(tier), 6), Tier::Generic { artifact: 6 }));
}

#[test]
fn a_stale_compile_counts_as_a_body_and_a_refused_one_does_not() {
    let compiling = |queued: T| queued.transition(Event::Started(Build::new(Box::new(0), 0)), &TIERS).0;
    let (tier, why) = compiling(warm(T::Cold)).transition(Event::STALE, &TIERS);
    assert!(matches!(&tier, Tier::Warming { past: Past { bodies: 1, .. }, .. }) && why == Why::Stale);
    let (tier, why) = compiling(warm(T::Cold)).transition(Event::REFUSED, &TIERS);
    assert!(matches!(&tier, Tier::Warming { entries: 0, past: Past { bodies: 0, .. }, .. }) && why == Why::Refused);
    let mut tier = T::Cold;
    for _ in 0..usize::from(SPECULATIVE_BODIES) + 2 {
        tier = compiling(warm(tier)).transition(Event::STALE, &TIERS).0;
    }
    assert!(matches!(&tier, Tier::Warming { past, .. } if past.is_spent()), "stale compiles cannot spend more than the budget");
}

#[test]
fn a_refused_build_runs_interpreted_and_is_revived_by_its_entries() {
    let (mut tier, why) = warm(T::Cold).transition(Event::Failed(Refusal::Shape("test")), &TIERS);
    assert!(matches!(tier, Tier::GenericInterpreted { entries: 0, .. }) && why == Why::Failed);
    for _ in 1..REVIVE {
        tier = enter(tier).0;
        assert!(matches!(tier, Tier::GenericInterpreted { .. }));
    }
    let (tier, why) = enter(tier);
    assert!(matches!(&tier, Tier::Warming { entries: REVIVE, past, .. } if past.is_spent()) && why == Why::Revived);
}

#[test]
fn a_codegen_failure_is_not_revived_by_entries_and_observed_entries_never_revive() {
    let (mut tier, _) = warm(T::Cold).transition(Event::Failed(Refusal::Codegen), &TIERS);
    for _ in 0..=REVIVE {
        tier = enter(tier).0;
    }
    assert!(matches!(tier, Tier::GenericInterpreted { refusal: Refusal::Codegen, .. }));
    let (mut tier, _) = T::Cold.transition(Event::Failed(Refusal::Shape("test")), &TIERS);
    for _ in 0..=REVIVE {
        tier = tier.transition(Event::OBSERVED, &TIERS).0;
    }
    assert!(matches!(tier, Tier::GenericInterpreted { entries, .. } if entries > REVIVE), "counted, not revived");
    let (tier, why) = tier.transition(Event::GENERALIZE, &TIERS);
    assert!(matches!(&tier, Tier::Warming { past, .. } if past.is_spent()) && why == Why::Revived, "the owner may revive it");
}

#[test]
fn the_owner_prices_the_warmup_and_a_refused_compile_warms_again_with_a_longer_wait() {
    let (tier, why) = T::Cold.transition(Event::DUE, &TIERS);
    assert!(matches!(tier, Tier::Queued { .. }) && why == Why::Warmed);
    let (tier, why) = tier.transition(Event::REFUSED, &TIERS);
    let Tier::Warming { entries: 0, past, .. } = &tier else { panic!("warms again") };
    assert_eq!((why, past.refused), (Why::Refused, 1));
    let (tier, _) = tier.transition(Event::DUE, &TIERS);
    let Tier::Queued { past } = tier else { panic!("queued") };
    assert_eq!(past.refused, 1, "the refusal outlives the warm-up");
}

#[test]
fn apply_moves_the_state_through_a_transition_in_place() {
    let mut tier = T::Cold;
    for _ in 0..WARMUP {
        tier.apply(Event::Entry { favourable: false }, &TIERS);
    }
    assert!(matches!(tier, Tier::Queued { .. }));
    assert_eq!(tier.apply(Event::Compiled(3), &TIERS), Why::Published);
    assert_eq!(tier.artifact(), Some(&3));
}
