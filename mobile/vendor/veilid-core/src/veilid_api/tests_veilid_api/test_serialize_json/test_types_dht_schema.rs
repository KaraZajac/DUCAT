use super::*;
use crate::crypto::tests_crypto::*;

// dlft

pub fn test_dht_schema_dflt() {
    let orig = DHTSchemaDFLT::new(9);
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

// mod

pub fn test_dht_schema() {
    let orig = DHTSchema::SMPL(
        DHTSchemaSMPL::new(
            91,
            vec![
                DHTSchemaSMPLMember {
                    m_key: fake_bare_member_id(),
                    m_cnt: 5,
                },
                DHTSchemaSMPLMember {
                    m_key: fake_bare_member_id(),
                    m_cnt: 6,
                },
            ],
        )
        .unwrap(),
    );
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

// smpl

pub fn test_dht_schema_smpl_member() {
    let orig = DHTSchemaSMPLMember {
        m_key: fake_bare_member_id(),
        m_cnt: 7,
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_dht_schema_smpl() {
    let orig = DHTSchemaSMPL::new(
        91,
        vec![
            DHTSchemaSMPLMember {
                m_key: fake_bare_member_id(),
                m_cnt: 8,
            },
            DHTSchemaSMPLMember {
                m_key: fake_bare_member_id(),
                m_cnt: 9,
            },
        ],
    )
    .unwrap();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}
