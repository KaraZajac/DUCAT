use super::*;

pub enum GlobalOnlineOfflineEvent {
    Online,
    Offline,
}

pub static GLOBAL_ONLINE_EVENT: LazyLock<
    Option<GlobalEventListenerWrapper<GlobalOnlineOfflineEvent>>,
> = LazyLock::new(|| {
    GlobalEventListenerWrapper::new(
        "online",
        Box::new(move |_| Some(GlobalOnlineOfflineEvent::Online)),
    )
    .ok_or_log()
});

pub static GLOBAL_OFFLINE_EVENT: LazyLock<
    Option<GlobalEventListenerWrapper<GlobalOnlineOfflineEvent>>,
> = LazyLock::new(|| {
    GlobalEventListenerWrapper::new(
        "offline",
        Box::new(move |_| Some(GlobalOnlineOfflineEvent::Offline)),
    )
    .ok_or_log()
});
