use crate::routing_table::*;

// 'Now' far enough from zero to subtract spans without underflow.
fn now() -> Timestamp {
    Timestamp::new(1_000_000_000u64) // 1000s, in microseconds
}
// A timestamp `secs` seconds before now().
fn secs_ago(secs: u64) -> Timestamp {
    Timestamp::new(now().as_u64() - secs * 1_000_000u64)
}
// Whole seconds in a span duration.
fn span_secs(d: TimestampDuration) -> u64 {
    d.as_u64() / 1_000_000u64
}
fn compute(
    punishment: Option<PunishmentReason>,
    rpc_stats: &RPCStats,
    per_so: &BTreeMap<SequenceOrdering, RPCStats>,
    per_transport: &BTreeMap<TransportType, RPCStats>,
) -> BucketEntryStateReason {
    BucketEntryInner::compute_state_reason_from_stats(
        now(),
        punishment,
        rpc_stats,
        per_so,
        per_transport,
        |_| TimestampDuration::new(0),
    )
}
// As `compute`, but credits `offline_secs` of offline time to every window.
fn compute_with_offline(
    punishment: Option<PunishmentReason>,
    rpc_stats: &RPCStats,
    per_so: &BTreeMap<SequenceOrdering, RPCStats>,
    per_transport: &BTreeMap<TransportType, RPCStats>,
    offline_secs: u32,
) -> BucketEntryStateReason {
    BucketEntryInner::compute_state_reason_from_stats(
        now(),
        punishment,
        rpc_stats,
        per_so,
        per_transport,
        |_| TimestampDuration::new_secs(offline_secs),
    )
}
fn so_map(entries: &[(SequenceOrdering, RPCStats)]) -> BTreeMap<SequenceOrdering, RPCStats> {
    entries.iter().cloned().collect()
}
// A non-empty per-transport map; only its emptiness gates [D3]/[D4], contents are irrelevant.
fn seen_transports() -> BTreeMap<TransportType, RPCStats> {
    let mut m = BTreeMap::new();
    m.insert(
        TransportType::new(ProtocolType::UDP, AddressType::IPV4),
        RPCStats::default(),
    );
    m
}

pub fn test_state_punished() {
    info!("--- test_state_punished ---");

    // [P1] punished beats everything, even an otherwise-reliable node
    let reliable = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_answer_ts: Some(secs_ago(120)),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(
            Some(PunishmentReason::Manual),
            &RPCStats::default(),
            &reliable,
            &seen_transports()
        ),
        BucketEntryStateReason::Punished(PunishmentReason::Manual)
    );
}

pub fn test_state_dead_excessive_unreachable() {
    info!("--- test_state_dead_excessive_unreachable ---");

    // [D1] a single node-level unreachable is dead. This also shadows [M1]
    // (unreachable > 0) while DEAD_UNREACHABLE_COUNT == 1.
    let rpc = RPCStats {
        unreachable: DEAD_UNREACHABLE_COUNT,
        ..Default::default()
    };
    assert_eq!(
        compute(None, &rpc, &BTreeMap::new(), &BTreeMap::new()),
        BucketEntryStateReason::Dead(BucketEntryStateDeadReason::ExcessiveUnreachable)
    );
}

pub fn test_state_dead_excessive_send_failures() {
    info!("--- test_state_dead_excessive_send_failures ---");

    // [D2] failed_to_send at/above threshold on any sequence ordering is dead
    for so in [SequenceOrdering::Unordered, SequenceOrdering::Ordered] {
        let per_so = so_map(&[(
            so,
            RPCStats {
                failed_to_send: dead_failed_to_send_count(so),
                ..Default::default()
            },
        )]);
        assert_eq!(
            compute(None, &RPCStats::default(), &per_so, &BTreeMap::new()),
            BucketEntryStateReason::Dead(BucketEntryStateDeadReason::ExcessiveSendFailures),
            "{so:?}"
        );
    }
}

pub fn test_state_dead_never_seen_lost_questions() {
    info!("--- test_state_dead_never_seen_lost_questions ---");

    // [D3] never-seen (no per-transport stats) with enough lost questions is dead
    for so in [SequenceOrdering::Unordered, SequenceOrdering::Ordered] {
        let per_so = so_map(&[(
            so,
            RPCStats {
                recent_lost_questions: dead_never_seen_lost_questions_count(so),
                ..Default::default()
            },
        )]);
        // per_transport empty == never seen
        assert_eq!(
            compute(None, &RPCStats::default(), &per_so, &BTreeMap::new()),
            BucketEntryStateReason::Dead(BucketEntryStateDeadReason::NeverSeenLostQuestions),
            "{so:?}"
        );
    }
}

pub fn test_state_dead_steady_lost_questions() {
    info!("--- test_state_dead_steady_lost_questions ---");

    // [D4] seen before, steadily losing questions past the span, is dead
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_lost_question_ts: Some(secs_ago(span_secs(DEAD_LOST_QUESTION_SPAN) + 1)),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Dead(BucketEntryStateDeadReason::SteadyLostQuestions)
    );

    // boundary: a steady lost question newer than the span is not yet dead
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_lost_question_ts: Some(secs_ago(span_secs(DEAD_LOST_QUESTION_SPAN) - 1)),
            ..Default::default()
        },
    )]);
    assert_ne!(
        BucketEntryState::from(compute(
            None,
            &RPCStats::default(),
            &per_so,
            &seen_transports()
        )),
        BucketEntryState::Dead,
        "should not be dead under the span"
    );
}

pub fn test_state_missing_failed_to_send() {
    info!("--- test_state_missing_failed_to_send ---");

    // [M2] some send failures, but below the dead threshold, is missing
    assert!(1 < dead_failed_to_send_count(SequenceOrdering::Unordered));
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            failed_to_send: 1,
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &BTreeMap::new()),
        BucketEntryStateReason::Missing(BucketEntryStateMissingReason::FailedToSend)
    );
}

pub fn test_state_missing_lost_questions() {
    info!("--- test_state_missing_lost_questions ---");

    // [M3] seen before, lost questions at/above the missing threshold but no
    // steady span yet, is missing. per_transport is non-empty so [D3] does not apply.
    for so in [SequenceOrdering::Unordered, SequenceOrdering::Ordered] {
        let per_so = so_map(&[(
            so,
            RPCStats {
                recent_lost_questions: missing_lost_questions_count(so),
                first_steady_lost_question_ts: None,
                ..Default::default()
            },
        )]);
        assert_eq!(
            compute(None, &RPCStats::default(), &per_so, &seen_transports()),
            BucketEntryStateReason::Missing(BucketEntryStateMissingReason::LostQuestions),
            "{so:?}"
        );
    }
}

pub fn test_state_initial() {
    info!("--- test_state_initial ---");

    // [I1] cold start (no per-sequence-ordering stats at all) is initial
    assert_eq!(
        compute(
            None,
            &RPCStats::default(),
            &BTreeMap::new(),
            &BTreeMap::new()
        ),
        BucketEntryStateReason::Initial
    );

    // a sequence ordering tracked but never answered is still initial
    let per_so = so_map(&[(SequenceOrdering::Unordered, RPCStats::default())]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Initial
    );
}

pub fn test_state_unreliable() {
    info!("--- test_state_unreliable ---");

    // [U1] a recent first steady answer (within the span) is unreliable
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_answer_ts: Some(secs_ago(span_secs(UNRELIABLE_ANSWER_SPAN) - 1)),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Unreliable
    );
}

pub fn test_state_reliable() {
    info!("--- test_state_reliable ---");

    // [R1] a first steady answer older than the span, with no failures, is reliable
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_answer_ts: Some(secs_ago(span_secs(UNRELIABLE_ANSWER_SPAN) + 1)),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Reliable
    );
}

pub fn test_state_precedence_dead_beats_reliable() {
    info!("--- test_state_precedence_dead_beats_reliable ---");

    // dead on one ordering kills the node even if another ordering is reliable
    let per_so = so_map(&[
        (
            SequenceOrdering::Unordered,
            RPCStats {
                first_steady_answer_ts: Some(secs_ago(120)),
                ..Default::default()
            },
        ),
        (
            SequenceOrdering::Ordered,
            RPCStats {
                failed_to_send: dead_failed_to_send_count(SequenceOrdering::Ordered),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Dead(BucketEntryStateDeadReason::ExcessiveSendFailures)
    );
}

pub fn test_state_transition_reliable_degrades_to_missing() {
    info!("--- test_state_transition_reliable_degrades_to_missing ---");

    // a previously-reliable node that starts losing questions degrades to missing
    let per_so = so_map(&[(
        SequenceOrdering::Unordered,
        RPCStats {
            first_steady_answer_ts: Some(secs_ago(120)),
            recent_lost_questions: missing_lost_questions_count(SequenceOrdering::Unordered),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Missing(BucketEntryStateMissingReason::LostQuestions)
    );
}

pub fn test_state_offline_credit_steady_lost_questions() {
    info!("--- test_state_offline_credit_steady_lost_questions ---");

    // [D4] a node losing questions past the dead span is Dead by raw time.
    let per_so = so_map(&[(
        SequenceOrdering::Ordered,
        RPCStats {
            first_steady_lost_question_ts: Some(secs_ago(span_secs(DEAD_LOST_QUESTION_SPAN) + 10)),
            recent_lost_questions: missing_lost_questions_count(SequenceOrdering::Ordered),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Dead(BucketEntryStateDeadReason::SteadyLostQuestions)
    );
    // Crediting offline time pulls the effective span under the threshold: not Dead.
    assert_eq!(
        compute_with_offline(None, &RPCStats::default(), &per_so, &seen_transports(), 20),
        BucketEntryStateReason::Missing(BucketEntryStateMissingReason::LostQuestions)
    );
    // A genuinely long lost span stays Dead despite the same offline credit.
    let long_lost = so_map(&[(
        SequenceOrdering::Ordered,
        RPCStats {
            first_steady_lost_question_ts: Some(secs_ago(span_secs(DEAD_LOST_QUESTION_SPAN) + 200)),
            recent_lost_questions: missing_lost_questions_count(SequenceOrdering::Ordered),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute_with_offline(
            None,
            &RPCStats::default(),
            &long_lost,
            &seen_transports(),
            20
        ),
        BucketEntryStateReason::Dead(BucketEntryStateDeadReason::SteadyLostQuestions)
    );
}

pub fn test_state_offline_credit_unreliable_answer() {
    info!("--- test_state_offline_credit_unreliable_answer ---");

    // [R1] a node answering past the unreliable span is Reliable by raw time.
    let per_so = so_map(&[(
        SequenceOrdering::Ordered,
        RPCStats {
            first_steady_answer_ts: Some(secs_ago(span_secs(UNRELIABLE_ANSWER_SPAN) + 10)),
            ..Default::default()
        },
    )]);
    assert_eq!(
        compute(None, &RPCStats::default(), &per_so, &seen_transports()),
        BucketEntryStateReason::Reliable
    );
    // Crediting offline time pulls the effective answer span under the threshold:
    // still Unreliable (we don't grant reliability for time we couldn't test).
    assert_eq!(
        compute_with_offline(None, &RPCStats::default(), &per_so, &seen_transports(), 20),
        BucketEntryStateReason::Unreliable
    );
}
