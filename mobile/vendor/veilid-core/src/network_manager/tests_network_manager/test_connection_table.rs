use super::connection_table::*;
use super::*;
use crate::tests::*;

pub async fn test_add_get_remove() {
    let registry = mock_registry::init("").await;

    let table = ConnectionTable::new(registry.clone());

    let a1 = Flow::new_no_local(PeerAddress::new(
        SocketAddress::new(Address::IPV4(Ipv4Addr::new(192, 168, 0, 1)), 8080),
        ProtocolType::TCP,
    ));
    let a2 = a1;
    let a3 = Flow::new(
        PeerAddress::new(
            SocketAddress::new(Address::IPV6(Ipv6Addr::new(191, 0, 0, 0, 0, 0, 0, 1)), 8090),
            ProtocolType::TCP,
        ),
        SocketAddress::from_socket_addr(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(191, 0, 0, 0, 0, 0, 0, 1),
            8080,
            0,
            0,
        ))),
    );
    let a4 = Flow::new(
        PeerAddress::new(
            SocketAddress::new(Address::IPV6(Ipv6Addr::new(192, 0, 0, 0, 0, 0, 0, 1)), 8090),
            ProtocolType::TCP,
        ),
        SocketAddress::from_socket_addr(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(192, 0, 0, 0, 0, 0, 0, 1),
            8080,
            0,
            0,
        ))),
    );
    let a5 = Flow::new(
        PeerAddress::new(
            SocketAddress::new(Address::IPV6(Ipv6Addr::new(192, 0, 0, 0, 0, 0, 0, 1)), 8090),
            ProtocolType::WS,
        ),
        SocketAddress::from_socket_addr(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(193, 0, 0, 0, 0, 0, 0, 1),
            8080,
            0,
            0,
        ))),
    );

    let c1 = NetworkConnection::dummy(registry.clone(), 1.into(), a1);
    let c1b = NetworkConnection::dummy(registry.clone(), 10.into(), a1);
    let c1h = c1.get_handle();
    let c2 = NetworkConnection::dummy(registry.clone(), 2.into(), a2);
    let c3 = NetworkConnection::dummy(registry.clone(), 3.into(), a3);
    let c4 = NetworkConnection::dummy(registry.clone(), 4.into(), a4);
    let c5 = NetworkConnection::dummy(registry.clone(), 5.into(), a5);

    assert_eq!(a1, c2.flow());
    assert_ne!(a3, c4.flow());
    assert_ne!(a4, c5.flow());

    assert_eq!(table.connection_count(), 0);
    assert_eq!(table.peek_connection_by_flow(a1), None);
    table.add_connection(c1).unwrap();
    assert!(table.add_connection(c1b).is_err());

    assert_eq!(table.connection_count(), 1);
    assert!(table.remove_connection_by_id(4.into()).is_none());
    assert!(table.remove_connection_by_id(5.into()).is_none());
    assert_eq!(table.connection_count(), 1);
    assert_eq!(table.peek_connection_by_flow(a1), Some(c1h.clone()));
    assert_eq!(table.peek_connection_by_flow(a1), Some(c1h.clone()));
    assert_eq!(table.connection_count(), 1);
    assert_err!(table.add_connection(c2));
    assert_eq!(table.connection_count(), 1);
    assert_eq!(table.peek_connection_by_flow(a1), Some(c1h.clone()));
    assert_eq!(table.peek_connection_by_flow(a1), Some(c1h.clone()));
    assert_eq!(table.connection_count(), 1);
    assert_eq!(
        table
            .remove_connection_by_id(1.into())
            .map(|c| c.flow())
            .unwrap(),
        a1
    );
    assert_eq!(table.connection_count(), 0);
    assert!(table.remove_connection_by_id(2.into()).is_none());
    assert_eq!(table.connection_count(), 0);
    assert_eq!(table.peek_connection_by_flow(a2), None);
    assert_eq!(table.peek_connection_by_flow(a1), None);
    assert_eq!(table.connection_count(), 0);
    let c1 = NetworkConnection::dummy(registry.clone(), 6.into(), a1);
    table.add_connection(c1).unwrap();
    let c2 = NetworkConnection::dummy(registry.clone(), 7.into(), a2);
    assert_err!(table.add_connection(c2));
    table.add_connection(c3).unwrap();
    table.add_connection(c4).unwrap();
    assert_eq!(table.connection_count(), 3);
    assert_eq!(
        table
            .remove_connection_by_id(6.into())
            .map(|c| c.flow())
            .unwrap(),
        a2
    );
    assert_eq!(
        table
            .remove_connection_by_id(3.into())
            .map(|c| c.flow())
            .unwrap(),
        a3
    );
    assert_eq!(
        table
            .remove_connection_by_id(4.into())
            .map(|c| c.flow())
            .unwrap(),
        a4
    );
    assert_eq!(table.connection_count(), 0);

    mock_registry::terminate(registry).await;
}

pub async fn test_dead_connection_skip() {
    let registry = mock_registry::init("conn_dead").await;

    let table = ConnectionTable::new(registry.clone());

    // Two connections to the same remote, distinct local addresses
    let remote_pa = PeerAddress::new(
        SocketAddress::new(Address::IPV4(Ipv4Addr::new(10, 1, 0, 1)), 8080),
        ProtocolType::TCP,
    );
    let local1 = SocketAddress::new(Address::IPV4(Ipv4Addr::new(10, 1, 0, 2)), 5001);
    let local2 = SocketAddress::new(Address::IPV4(Ipv4Addr::new(10, 1, 0, 2)), 5002);
    let f1 = Flow::new(remote_pa, local1);
    let f2 = Flow::new(remote_pa, local2);
    let remote = (
        ProtocolType::TCP.low_level_protocol_type(),
        *remote_pa.socket_address(),
    );

    let c1 = NetworkConnection::dummy(registry.clone(), 1.into(), f1);
    let c1h = c1.get_handle();
    let mut c2 = NetworkConnection::dummy(registry.clone(), 2.into(), f2);
    let c2h = c2.get_handle();

    // Dead connections refuse sends even through an existing handle
    c2.close();
    assert!(matches!(
        c2h.send_async(Bytes::new()).await,
        ConnectionHandleSendResult::NotSent(_)
    ));

    table.add_connection(c1).unwrap();
    table.add_connection(c2).unwrap();

    // Selectors skip the dead connection even though it is still in the table
    assert_eq!(
        table.get_best_connection_by_remote(None, remote),
        Some(c1h.clone())
    );
    assert_eq!(table.peek_connection_by_flow(f2), None);

    // Kill the last live connection; the remote no longer resolves
    table.remove_connection_by_id(1.into()).unwrap().close();
    assert_eq!(table.get_best_connection_by_remote(None, remote), None);
    assert!(matches!(
        c1h.send_async(Bytes::new()).await,
        ConnectionHandleSendResult::NotSent(_)
    ));

    mock_registry::terminate(registry).await;
}

// Unique flow per id; distinct remote IP keeps each connection separate and well
// under the per-ip connection limit.
fn flow_n(n: u64, protocol_type: ProtocolType) -> Flow {
    Flow::new_no_local(PeerAddress::new(
        SocketAddress::new(
            Address::IPV4(Ipv4Addr::new(10, 0, (n >> 8) as u8, (n & 0xff) as u8)),
            8080,
        ),
        protocol_type,
    ))
}

fn add_dummy(
    table: &ConnectionTable,
    registry: &VeilidComponentRegistry,
    id: u64,
    protocol_type: ProtocolType,
) -> Option<NetworkConnection> {
    table
        .add_connection(NetworkConnection::dummy(
            registry.clone(),
            id.into(),
            flow_n(id, protocol_type),
        ))
        .unwrap()
}

pub async fn test_eviction() {
    let registry = mock_registry::init("conn_evict").await;

    // Discover the config-driven capacity: fill unique connections until one overflows.
    let cap = {
        let table = ConnectionTable::new(registry.clone());
        let mut id = 0u64;
        loop {
            id += 1;
            if add_dummy(&table, &registry, id, ProtocolType::TCP).is_some() {
                break table.connection_count();
            }
            assert!(id < 100_000, "table never overflowed");
        }
    };
    assert!(cap >= 2, "capacity too small to test eviction");

    // Global LRU + cross-protocol fairness: the oldest connection (id 1, a WS) is the
    // eviction victim even though the newcomer and the rest of the table are TCP.
    // Eviction is by global recency, not partitioned per protocol.
    {
        let table = ConnectionTable::new(registry.clone());
        for id in 1..=cap as u64 {
            let protocol_type = if id == 1 {
                ProtocolType::WS
            } else {
                ProtocolType::TCP
            };
            assert!(add_dummy(&table, &registry, id, protocol_type).is_none());
        }
        let evicted = add_dummy(&table, &registry, cap as u64 + 1, ProtocolType::TCP)
            .expect("overflow should evict");
        assert_eq!(evicted.connection_id(), 1u64.into(), "global LRU victim");
        assert_eq!(table.connection_count(), cap);
    }

    // Recency: touching the oldest connection spares it; the next-oldest is evicted.
    {
        let table = ConnectionTable::new(registry.clone());
        for id in 1..=cap as u64 {
            assert!(add_dummy(&table, &registry, id, ProtocolType::TCP).is_none());
        }
        table.touch_connection_by_id(1u64.into());
        let evicted = add_dummy(&table, &registry, cap as u64 + 1, ProtocolType::TCP)
            .expect("overflow should evict");
        assert_eq!(
            evicted.connection_id(),
            2u64.into(),
            "LRU victim after touch"
        );
    }

    // Priority flows are protected from eviction (same skip-branch as protected
    // connections): the oldest survives and the next-oldest is evicted instead.
    {
        let table = ConnectionTable::new(registry.clone());
        let f1 = flow_n(1, ProtocolType::TCP);
        for id in 1..=cap as u64 {
            assert!(add_dummy(&table, &registry, id, ProtocolType::TCP).is_none());
        }
        table.add_priority_flow(f1);
        let evicted = add_dummy(&table, &registry, cap as u64 + 1, ProtocolType::TCP)
            .expect("overflow should evict");
        assert_eq!(
            evicted.connection_id(),
            2u64.into(),
            "priority flow id 1 must survive"
        );
        assert!(
            table.peek_connection_by_flow(f1).is_some(),
            "priority flow still present"
        );
    }

    mock_registry::terminate(registry).await;
}
