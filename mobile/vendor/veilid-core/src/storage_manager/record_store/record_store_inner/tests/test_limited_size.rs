use super::*;
use crate::tests::*;

pub async fn test_limited_size() {
    let registry = mock_registry::init("limited_size").await;

    test_no_ops(registry.clone());
    test_concurrent(registry.clone());
    test_modify_set_no_limit(registry.clone());
    test_modify_set_with_limit(registry.clone());
    test_modify_add_no_limit(registry.clone());
    test_modify_add_with_limit(registry.clone());
    test_modify_sub_no_limit(registry.clone());

    test_modify_add_sub_no_limit(registry.clone());
    test_modify_add_sub_with_limit(registry.clone());

    mock_registry::terminate(registry).await;
}

fn test_no_ops(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "new a", None, 0u32).expect("should succeed");
    assert_eq!(ls.limit(), None);
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let ls =
        LimitedSize::try_new(registry.clone(), "new b", Some(0), 0u32).expect("should succeed");
    assert_eq!(ls.limit(), Some(0));
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let _ =
        LimitedSize::try_new(registry.clone(), "new c", Some(0), 1u32).expect_err("should fail");

    let ls =
        LimitedSize::try_new(registry.clone(), "new d", Some(8), 0u32).expect("should succeed");
    assert_eq!(ls.limit(), Some(8));
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let ls =
        LimitedSize::try_new(registry.clone(), "new e", Some(8), 8u32).expect("should succeed");
    assert_eq!(ls.limit(), Some(8));
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let _ = LimitedSize::try_new(registry.clone(), "new f", Some(8), u32::MAX)
        .expect_err("should fail");

    let ls = LimitedSize::try_new(registry.clone(), "new g", Some(u32::MAX), 256u32)
        .expect("should succeed");
    assert_eq!(ls.limit(), Some(u32::MAX));
    assert_eq!(ls.with_value(|v| v).unwrap(), 256u32);

    let ls = LimitedSize::try_new(registry.clone(), "new h", Some(u32::MAX), u32::MAX)
        .expect("should succeed");
    assert_eq!(ls.limit(), Some(u32::MAX));
    assert_eq!(ls.with_value(|v| v).unwrap(), u32::MAX);
}

fn test_concurrent(registry: VeilidComponentRegistry) {
    let ls =
        LimitedSize::try_new(registry.clone(), "concurrent a", None, 0u32).expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    assert_eq!(
        ls.with_value(|v| {
            ls.with_value(|w| w)
                .expect_err("should fail on concurrent access");
            let _ = ls.modify().expect_err("should fail on concurrent modify");
            v
        })
        .unwrap(),
        0u32
    );
    {
        let lsg = ls.modify().expect("should modify");
        ls.with_value(|v| v)
            .expect_err("should fail on concurrent access");
        let _ = ls.modify().expect_err("should fail on concurrent modify");
        lsg.commit().expect("should commit");
    }
    {
        let mut lsg = ls.modify().expect("should modify");
        lsg.add(3).expect("should add");
        // drop lsg without commit or rollback
    }
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);
}

fn test_modify_set_no_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify set no limit", None, 8u32)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.set(0);
    assert!(lsg.check_limit(), "should be true");
    lsg.set(1);
    assert!(lsg.check_limit(), "should be true");
    lsg.set(9);
    assert!(lsg.check_limit(), "should be true");
    lsg.set(8);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");

    let mut lsg = ls.modify().unwrap();
    lsg.set(0);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(8);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(9);
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 9u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(2);
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 9u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(9);
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 9u32);

    let lsg = ls.modify().unwrap();
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 9u32);
}

fn test_modify_set_with_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify set with limit", Some(8), 8u32)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.set(0);
    assert!(lsg.check_limit(), "should be true");
    lsg.set(1);
    assert!(lsg.check_limit(), "should be true");
    lsg.set(9);
    assert!(!lsg.check_limit(), "should be false");
    lsg.set(8);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");

    let mut lsg = ls.modify().unwrap();
    lsg.set(0);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(8);
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(9);
    lsg.commit().expect_err("should fail to commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(2);
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let mut lsg = ls.modify().unwrap();
    lsg.set(9);
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);

    let lsg = ls.modify().unwrap();
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);
}

fn test_modify_add_no_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify add no limit", None, 0u32)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.add(0).expect("should add");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(1).expect("should add");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(u32::MAX).expect_err("should fail");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(u32::MAX - 1).expect("should succeed");
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), u32::MAX);
}

fn test_modify_add_with_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify add with limit", Some(8), 0u32)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.add(0).expect("should add");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(1).expect("should add");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(u32::MAX).expect_err("should fail");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(u32::MAX - 1).expect("should succeed");
    assert!(!lsg.check_limit(), "should be false");
    lsg.commit().expect_err("should not commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.add(0).expect("should add");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.add(9).expect("should add");
    lsg.commit().expect_err("should not commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.add(1).expect("should add");
    lsg.rollback();
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.add(1).expect("should add");
    lsg.add(7).expect("should add");
    lsg.commit().expect("should commit");
    assert_eq!(ls.with_value(|v| v).unwrap(), 8u32);
}

fn test_modify_sub_no_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify sub no limit", None, u32::MAX)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(0).expect("should sub"); // u32::MAX
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(1).expect("should sub"); // u32::MAX - 1
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(u32::MAX).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.sub(u32::MAX - 1).expect("should succeed"); // -u32::MAX
    assert!(!lsg.check_limit(), "should be false");
    lsg.commit().expect_err("should fail");

    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(u32::MAX).expect("should sub"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(1).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.commit().expect_err("should fail");
}

fn test_modify_add_sub_no_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify add sub no limit", None, 0)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(0).expect("should sub"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(1).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.sub(u32::MAX).expect_err("should fail");
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(1).expect("should add"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.sub(1).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(u32::MAX).expect("should succeed"); // u32::MAX-1
    assert!(lsg.check_limit(), "should be true");
    lsg.add(u32::MAX).expect_err("should fail");
    assert!(lsg.check_limit(), "should be true");
    lsg.add(1).expect("should succeed"); // u32::MAX
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), u32::MAX);

    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(u32::MAX).expect("should sub"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(u32::MAX).expect("should sub"); // -u32::MAX
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(u32::MAX - 1).expect("should add"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(2).expect("should add"); // 1
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 1);
}

fn test_modify_add_sub_with_limit(registry: VeilidComponentRegistry) {
    let ls = LimitedSize::try_new(registry.clone(), "modify add sub with limit", Some(8), 0)
        .expect("should succeed");
    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(0).expect("should sub"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(1).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.sub(u32::MAX).expect_err("should fail");
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(1).expect("should add"); // 0
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0u32);

    let mut lsg = ls.modify().unwrap();
    lsg.sub(1).expect("should sub"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(u32::MAX).expect("should succeed"); // u32::MAX-1
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(u32::MAX).expect_err("should fail");
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(1).expect("should succeed"); // u32::MAX
    assert!(!lsg.check_limit(), "should be false");
    lsg.commit().expect_err("should fail");
    assert_eq!(ls.with_value(|v| v).unwrap(), 0);

    let mut lsg = ls.modify().unwrap();
    assert!(lsg.check_limit(), "should be true");
    lsg.sub(u32::MAX).expect("should sub"); // -u32::MAX
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(u32::MAX - 1).expect("should add"); // -1
    assert!(!lsg.check_limit(), "should be false");
    lsg.add(9).expect("should add"); // 8
    assert!(lsg.check_limit(), "should be true");
    lsg.commit().expect("should succeed");
    assert_eq!(ls.with_value(|v| v).unwrap(), 8);
}
