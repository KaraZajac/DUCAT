//! §16.20's ask across the bridge: a kind-16 sealed on one side and opened
//! on the other still names the period it wanted.
//!
//! Core's own tests and the vectors pin the *encoding*; what they cannot see
//! is this crate's `seal_message`, where a field slotted into the wrong
//! place compiles perfectly and silently sends the wrong thing — or
//! nothing. `wanted_period` rides inside `PublicationSend` with six other
//! optional fields, any of which it could be confused with.

use ducat_mobile::contacts::{
    generate_prekeys, open_message, seal_message, thread_aad, PublicationSend,
};

/// The arguments, with everything the ask does not use left empty. Named
/// rather than inlined so a reordering shows up as a compile error here
/// instead of a wrong field on the wire.
#[allow(clippy::too_many_arguments)]
fn seal_ask(
    bundle: Vec<u8>,
    aad: Vec<u8>,
    kind: u8,
    wanted: Option<String>,
) -> Result<ducat_mobile::contacts::SealedOut, ducat_mobile::contacts::ContactError> {
    seal_message(
        bundle,
        0,           // seq
        vec![0; 32], // prev_link: the thread's genesis
        "Could I get issue-12?".to_string(),
        aad,
        kind,
        None,       // amount_pxmr
        None,       // txid
        None,       // payto
        Vec::new(), // items
        None,       // tax_pxmr
        None,       // re_seq
        false,      // re_own
        None,       // attachment
        None,       // eta_secs
        None,       // payload
        None,       // round
        None,       // ceremony_id
        None,       // position
        None,       // group
        wanted.map(|w| PublicationSend {
            period_id: None,
            period_key: None,
            record_key: None,
            head_key: None,
            swarm_key: None,
            swarm_digest: None,
            wanted_period: Some(w),
        }),
        None, // call
    )
}

#[test]
fn an_ask_crosses_the_bridge_with_its_period() {
    let keys = generate_prekeys(4, 86_400, 1, None);
    let aad = thread_aad("aa".repeat(32), "bb".repeat(32));

    let sealed = seal_ask(
        keys.bundle.clone(),
        aad.clone(),
        16,
        Some("issue-12".to_string()),
    )
    .expect("a kind-16 naming a period seals");

    // Opened with the one-time secret the seal consumed.
    let idx = keys
        .one_time_ids
        .iter()
        .position(|id| *id == sealed.prekey_id)
        .expect("the seal named a one-time key we published");
    let opened = open_message(
        sealed.bytes,
        keys.one_time_secrets[idx].clone(),
        true,
        0,
        Some(vec![0; 32]),
        aad.clone(),
    )
    .expect("the ask opens");

    assert_eq!(opened.kind, 16, "the kind survived");
    assert_eq!(
        opened.wanted_period.as_deref(),
        Some("issue-12"),
        "the period a reader asked for did not cross the bridge — check \
         wanted_period's position in seal_message's argument list",
    );
    // The ask hands over nothing. If a period *key* ever appeared here, the
    // reader would be describing a capability nobody granted them.
    assert!(
        opened.publication.is_none(),
        "an ask must carry no capability",
    );
}

#[test]
fn the_closed_world_holds_across_the_bridge() {
    let keys = generate_prekeys(4, 86_400, 1, None);
    let aad = thread_aad("aa".repeat(32), "bb".repeat(32));

    // A kind-16 with nothing to ask for is malformed: an ask that names no
    // period is not a request, it is noise.
    assert!(
        seal_ask(keys.bundle.clone(), aad.clone(), 16, None).is_err(),
        "a kind-16 with no period was accepted",
    );

    // ...and the period never rides another kind. Core would refuse such a
    // message outright — the `publication_wanted_on_a_text` vector pins
    // that on both implementations — but this bridge drops the field first
    // (`if kind == 16`), so core never sees it. That belt-and-braces is
    // worth pinning on its own: it is what stops a caller passing the two
    // arguments in the wrong order from building a message core would
    // reject, and it means the refusal a peer sees is the wire's, not ours.
    let text = seal_ask(
        keys.bundle.clone(),
        aad.clone(),
        0,
        Some("issue-12".to_string()),
    )
    .expect("a text still seals");
    let idx = keys
        .one_time_ids
        .iter()
        .position(|id| *id == text.prekey_id)
        .expect("published one-time key");
    let opened = open_message(
        text.bytes,
        keys.one_time_secrets[idx].clone(),
        true,
        0,
        Some(vec![0; 32]),
        aad,
    )
    .expect("the text opens");
    assert_eq!(
        opened.wanted_period, None,
        "a wanted period rode a kind that has no business carrying one",
    );
}
