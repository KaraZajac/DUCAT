use super::*;
use igd::*;
use std::net::UdpSocket;
use stop_token::future::FutureExt as StopFutureExt;

impl_veilid_log_facility!("net");

const UPNP_GATEWAY_DETECT_TIMEOUT_MS: u32 = 3_000;
const UPNP_MAPPING_ATTEMPTS: u32 = 3;
const UPNP_MAPPING_LIFETIME: TimestampDuration = TimestampDuration::new_ms(120_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PortMapKey {
    protocol_type: IGDProtocolType,
    address_type: IGDAddressType,
    local_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PortMapValue {
    ext_ip: IpAddr,
    mapped_port: u16,
    timestamp: Timestamp,
    renewal_lifetime: TimestampDuration,
    renewal_attempts: u32,
}

/// Command for the background worker
enum IGDCommand {
    Probe(IGDAddressType),
}

struct IGDManagerInner {
    /// Routed local IP per address type; `None` value = probed, no routable IP
    local_ip_addrs: BTreeMap<IGDAddressType, Option<IpAddr>>,
    /// IGD gateway per local IP; `None` value = probed, no gateway
    gateways: BTreeMap<IpAddr, Option<Arc<Gateway>>>,
    /// External IP per local IP; `None` value = probed, gateway gave no external IP
    external_ips: BTreeMap<IpAddr, Option<IpAddr>>,
    /// Active port mappings
    port_maps: BTreeMap<PortMapKey, PortMapValue>,
    /// Worker holds during `do_probe`; cache readers lock to wait for fresh cache
    probe_locks: BTreeMap<IGDAddressType, Arc<AsyncMutex<()>>>,
    /// Worker command sender
    cmd_tx: Option<flume::Sender<IGDCommand>>,
    /// Worker stop source
    stop_source: Option<StopSource>,
    /// Worker join handle
    worker_jh: Option<MustJoinHandle<()>>,
}

impl Drop for IGDManagerInner {
    fn drop(&mut self) {
        // Detach so MustJoinHandle::drop doesn't panic if shutdown() didn't run
        if let Some(jh) = self.worker_jh.take() {
            jh.detach();
        }
    }
}

#[derive(Clone)]
pub struct IGDManager {
    registry: VeilidComponentRegistry,
    inner: Arc<Mutex<IGDManagerInner>>,
}

impl_veilid_component_accessors!(IGDManager);

fn convert_protocol_type(igdpt: IGDProtocolType) -> PortMappingProtocol {
    match igdpt {
        IGDProtocolType::UDP => PortMappingProtocol::UDP,
        IGDProtocolType::TCP => PortMappingProtocol::TCP,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IGDAddressType {
    IPV6,
    IPV4,
}

impl fmt::Display for IGDAddressType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IGDAddressType::IPV6 => write!(f, "IPV6"),
            IGDAddressType::IPV4 => write!(f, "IPV4"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IGDProtocolType {
    UDP,
    TCP,
}

impl fmt::Display for IGDProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IGDProtocolType::UDP => write!(f, "UDP"),
            IGDProtocolType::TCP => write!(f, "TCP"),
        }
    }
}

impl IGDManager {
    /////////////////////////////////////////////////////////////////////
    // Public Interface

    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Arc::new(Mutex::new(IGDManagerInner {
                local_ip_addrs: BTreeMap::new(),
                gateways: BTreeMap::new(),
                external_ips: BTreeMap::new(),
                port_maps: BTreeMap::new(),
                probe_locks: BTreeMap::new(),
                cmd_tx: None,
                stop_source: None,
                worker_jh: None,
            })),
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[expect(dead_code)]
    pub async fn unmap_port(
        &self,
        protocol_type: IGDProtocolType,
        address_type: IGDAddressType,
        mapped_port: u16,
    ) -> Option<()> {
        // Wait for any in-flight probe for this address type so the cache below is fresh
        let _probe_guard = self.wait_for_probe(address_type).await;

        let this = self.clone();
        blocking_wrapper(move || {
            let mut inner = this.inner.lock();

            // If we already have this port mapped, just return the existing portmap
            let mut found = None;
            for (pmk, pmv) in &inner.port_maps {
                if pmk.protocol_type == protocol_type
                    && pmk.address_type == address_type
                    && pmv.mapped_port == mapped_port
                {
                    found = Some(*pmk);
                    break;
                }
            }
            let pmk = found?;
            let _pmv = inner
                .port_maps
                .remove(&pmk)
                .expect_or_log("key found but remove failed");

            // Get local ip address
            let local_ip = Self::find_local_ip_inner(&inner, address_type)?;

            // Find gateway
            let gw = Self::find_gateway_inner(&inner, local_ip)?;

            // Unmap port
            match gw.remove_port(convert_protocol_type(protocol_type), mapped_port) {
                Ok(()) => (),
                Err(e) => {
                    // Failed to map external port
                    veilid_log!(this debug "upnp failed to remove external port: {}", e);
                    return None;
                }
            };
            Some(())
        })
        .await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn map_any_port(
        &self,
        protocol_type: IGDProtocolType,
        address_type: IGDAddressType,
        local_port: u16,
        expected_external_address: Option<IpAddr>,
    ) -> Option<SocketAddr> {
        // Hold the probe lock across the blocking_wrapper so a probe can't repopulate
        // the cache mid-mapping
        let _probe_guard = self.wait_for_probe(address_type).await;

        let this = self.clone();
        blocking_wrapper(move || {
            let mut inner = this.inner.lock();

            let pmkey = PortMapKey {
                protocol_type,
                address_type,
                local_port,
            };
            if let Some(pmval) = inner.port_maps.get(&pmkey) {
                return Some(SocketAddr::new(pmval.ext_ip, pmval.mapped_port));
            }

            let local_ip = Self::find_local_ip_inner(&inner, address_type)?;
            let gw = Self::find_gateway_inner(&inner, local_ip)?;
            let ext_ip = Self::find_external_ip_inner(&inner, local_ip)?;

            // Ensure external IP matches address type
            if ext_ip.is_ipv4() && address_type != IGDAddressType::IPV4 {
                veilid_log!(this debug "mismatched ip address type from igd, wanted v4, got v6");
                return None;
            } else if ext_ip.is_ipv6() && address_type != IGDAddressType::IPV6 {
                veilid_log!(this debug "mismatched ip address type from igd, wanted v6, got v4");
                return None;
            }

            if let Some(expected_external_address) = expected_external_address {
                if ext_ip != expected_external_address {
                    veilid_log!(this debug "gateway external address does not match calculated external address: expected={} vs gateway={}", expected_external_address, ext_ip);
                    return None;
                }
            }

            // Map any port
            let desc = this.get_description(protocol_type, local_port);
            let mapped_port = match gw.add_any_port(convert_protocol_type(protocol_type), SocketAddr::new(local_ip, local_port), UPNP_MAPPING_LIFETIME.millis_u32().unwrap_or_log().div_ceil(1000), &desc) {
                Ok(mapped_port) => mapped_port,
                Err(e) => {
                    // Failed to map external port
                    veilid_log!(this debug "upnp failed to map external port: {}", e);
                    return None;
                }
            };

            // Add to mapping list to keep alive
            let timestamp = Timestamp::now();
            inner.port_maps.insert(PortMapKey {
                protocol_type,
                address_type,
                local_port,
            }, PortMapValue {
                ext_ip,
                mapped_port,
                timestamp,
                renewal_lifetime: UPNP_MAPPING_LIFETIME.div(2),
                renewal_attempts: 0,
            });

            // Succeeded, return the externally mapped port
            Some(SocketAddr::new(ext_ip, mapped_port))
        })
        .await
    }

    /// Spawn the background worker. Idempotent.
    pub fn startup(&self) {
        let mut inner = self.inner.lock();
        if inner.cmd_tx.is_some() {
            return;
        }
        let (tx, rx) = flume::unbounded::<IGDCommand>();
        let stop_source = StopSource::new();
        let stop_token = stop_source.token();
        let this = self.clone();
        let jh = spawn("igd worker", async move {
            this.worker_loop(rx, stop_token).await;
        });
        inner.cmd_tx = Some(tx);
        inner.stop_source = Some(stop_source);
        inner.worker_jh = Some(jh);
    }

    /// Stop the background worker and wait for it to finish. Idempotent.
    pub async fn shutdown(&self) {
        let jh = {
            let mut inner = self.inner.lock();
            inner.cmd_tx = None;
            inner.stop_source = None;
            inner.worker_jh.take()
        };
        if let Some(jh) = jh {
            jh.await;
        }
    }

    /// Drop cached probe results and active port mappings; used on interface change
    pub fn clear_caches(&self) {
        let mut inner = self.inner.lock();
        inner.local_ip_addrs.clear();
        inner.gateways.clear();
        inner.external_ips.clear();
        inner.port_maps.clear();
    }

    /// Asynchronously probe for the given address type; returns immediately.
    /// No-op before startup
    pub fn trigger_probe(&self, address_type: IGDAddressType) {
        let mut inner = self.inner.lock();
        inner
            .probe_locks
            .entry(address_type)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())));
        if let Some(tx) = inner.cmd_tx.as_ref() {
            if let Err(e) = tx.send(IGDCommand::Probe(address_type)) {
                veilid_log!(self debug "failed to send IGD probe command: {}", e);
            }
        }
    }

    /// Acquire the per-AT probe lock so callers can read caches knowing no probe
    /// will mutate them while the guard is held. `None` if no probe ever triggered.
    async fn wait_for_probe(&self, address_type: IGDAddressType) -> Option<AsyncMutexGuardArc<()>> {
        let lock = self.inner.lock().probe_locks.get(&address_type).cloned();
        match lock {
            Some(lock) => Some(lock.lock_arc().await),
            None => None,
        }
    }

    async fn worker_loop(&self, rx: flume::Receiver<IGDCommand>, stop_token: StopToken) {
        loop {
            let cmd = match rx.recv_async().timeout_at(stop_token.clone()).await {
                Ok(Ok(cmd)) => cmd,
                _ => return,
            };
            match cmd {
                IGDCommand::Probe(address_type) => {
                    let lock = self.inner.lock().probe_locks.get(&address_type).cloned();
                    let Some(lock) = lock else {
                        continue;
                    };
                    // Wrap awaits in stop_token; spawn_blocking inside do_probe can't be
                    // cancelled mid-flight, but dropping our await still lets the worker exit
                    let Ok(_guard) = lock.lock_arc().timeout_at(stop_token.clone()).await else {
                        return;
                    };
                    if self
                        .do_probe(address_type)
                        .timeout_at(stop_token.clone())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    }

    /// Probe local IP, gateway, and external IP for an address type and cache the result
    async fn do_probe(&self, address_type: IGDAddressType) {
        let this = self.clone();
        blocking_wrapper(move || {
            let local_ip = this.get_routed_local_ip_address(address_type);
            this.inner
                .lock()
                .local_ip_addrs
                .insert(address_type, local_ip);

            let Some(local_ip) = local_ip else {
                veilid_log!(this debug "no routable local ip for address_type={:?}", address_type);
                return;
            };

            let gateway_opt = match local_ip {
                IpAddr::V4(v4) => {
                    let mut opts = SearchOptions::new_v4(UPNP_GATEWAY_DETECT_TIMEOUT_MS as u64);
                    opts.bind_addr = SocketAddr::V4(SocketAddrV4::new(v4, 0));
                    match igd::search_gateway(opts) {
                        Ok(v) => Some(Arc::new(v)),
                        Err(e) => {
                            veilid_log!(this debug "couldn't find ipv4 igd: {}", e);
                            None
                        }
                    }
                }
                IpAddr::V6(v6) => {
                    let mut opts = SearchOptions::new_v6(
                        Ipv6SearchScope::LinkLocal,
                        UPNP_GATEWAY_DETECT_TIMEOUT_MS as u64,
                    );
                    opts.bind_addr = SocketAddr::V6(SocketAddrV6::new(v6, 0, 0, 0));
                    match igd::search_gateway(opts) {
                        Ok(v) => Some(Arc::new(v)),
                        Err(e) => {
                            veilid_log!(this debug "couldn't find ipv6 igd: {}", e);
                            None
                        }
                    }
                }
            };

            let ext_ip = gateway_opt
                .as_ref()
                .and_then(|gw| match gw.get_external_ip() {
                    Ok(ip) => Some(ip),
                    Err(e) => {
                        veilid_log!(this debug "couldn't get external ip from igd: {}", e);
                        None
                    }
                });

            let mut inner = this.inner.lock();
            inner.gateways.insert(local_ip, gateway_opt);
            inner.external_ips.insert(local_ip, ext_ip);
        })
        .await;
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(
            level = "trace",
            target = "net",
            name = "IGDManager::tick",
            skip_all,
            err,
            fields(__VEILID_LOG_KEY = self.log_key())
        )
    )]
    pub async fn tick(&self) -> EyreResult<bool> {
        // Refresh mappings if we have them
        // If an error is received, then return false to restart the local network
        let mut full_renews: Vec<(PortMapKey, PortMapValue)> = Vec::new();
        let mut renews: Vec<(PortMapKey, PortMapValue)> = Vec::new();
        {
            let inner = self.inner.lock();
            let now = Timestamp::now();

            for (k, v) in &inner.port_maps {
                let mapping_lifetime = now.duration_since(v.timestamp);
                if mapping_lifetime >= UPNP_MAPPING_LIFETIME
                    || v.renewal_attempts >= UPNP_MAPPING_ATTEMPTS
                {
                    // Past expiration time or tried N times, do a full renew and fail out if we can't
                    full_renews.push((*k, *v));
                } else if mapping_lifetime >= v.renewal_lifetime {
                    // Attempt a normal renewal
                    renews.push((*k, *v));
                }
            }

            // See if we need to do some blocking operations
            if full_renews.is_empty() && renews.is_empty() {
                // Just return now since there's nothing to renew
                return Ok(true);
            }
        }

        let this = self.clone();
        let fut = blocking_wrapper(move || {
            let mut inner = this.inner.lock();

            // Process full renewals
            for (k, v) in full_renews {
                // Get local ip for address type
                let local_ip = match Self::find_local_ip_inner(&inner, k.address_type) {
                    Some(ip) => ip,
                    None => {
                        return Err(eyre!("local ip missing for address type"));
                    }
                };

                // Get gateway for interface
                let gw = match Self::find_gateway_inner(&inner, local_ip) {
                    Some(gw) => gw,
                    None => {
                        return Err(eyre!("gateway missing for interface"));
                    }
                };

                // Delete the mapping if it exists, ignore any errors here
                let _ = gw.remove_port(convert_protocol_type(k.protocol_type), v.mapped_port);
                inner.port_maps.remove(&k);

                let desc = this.get_description(k.protocol_type, k.local_port);
                match gw.add_any_port(
                    convert_protocol_type(k.protocol_type),
                    SocketAddr::new(local_ip, k.local_port),
                    UPNP_MAPPING_LIFETIME
                        .millis_u32()
                        .unwrap_or_log()
                        .div_ceil(1000),
                    &desc,
                ) {
                    Ok(mapped_port) => {
                        veilid_log!(this debug "full-renewed mapped port {:?} -> {:?}", v, k);
                        inner.port_maps.insert(
                            k,
                            PortMapValue {
                                ext_ip: v.ext_ip,
                                mapped_port,
                                timestamp: Timestamp::now(),
                                renewal_lifetime: UPNP_MAPPING_LIFETIME.div(2),
                                renewal_attempts: 0,
                            },
                        );
                    }
                    Err(e) => {
                        veilid_log!(this info "failed to full-renew mapped port {:?} -> {:?}: {}", v, k, e);

                        // Must restart network now :(
                        return Ok(false);
                    }
                };
            }
            // Process normal renewals
            for (k, mut v) in renews {
                // Get local ip for address type
                let local_ip = match Self::find_local_ip_inner(&inner, k.address_type) {
                    Some(ip) => ip,
                    None => {
                        return Err(eyre!("local ip missing for address type"));
                    }
                };

                // Get gateway for interface
                let gw = match Self::find_gateway_inner(&inner, local_ip) {
                    Some(gw) => gw,
                    None => {
                        return Err(eyre!("gateway missing for address type"));
                    }
                };

                let desc = this.get_description(k.protocol_type, k.local_port);
                match gw.add_port(
                    convert_protocol_type(k.protocol_type),
                    v.mapped_port,
                    SocketAddr::new(local_ip, k.local_port),
                    UPNP_MAPPING_LIFETIME
                        .millis_u32()
                        .unwrap_or_log()
                        .div_ceil(1000),
                    &desc,
                ) {
                    Ok(()) => {
                        veilid_log!(this trace "renewed mapped port {:?} -> {:?}", v, k);

                        inner.port_maps.insert(
                            k,
                            PortMapValue {
                                ext_ip: v.ext_ip,
                                mapped_port: v.mapped_port,
                                timestamp: Timestamp::now(),
                                renewal_lifetime: UPNP_MAPPING_LIFETIME.div(2),
                                renewal_attempts: 0,
                            },
                        );
                    }
                    Err(e) => {
                        veilid_log!(this debug "failed to renew mapped port {:?} -> {:?}: {}", v, k, e);

                        // Get closer to the maximum renewal timeline by a factor of two each time
                        v.renewal_lifetime =
                            (v.renewal_lifetime.saturating_add(UPNP_MAPPING_LIFETIME)).div(2);
                        v.renewal_attempts += 1;

                        // Store new value to try again
                        inner.port_maps.insert(k, v);
                    }
                };
            }

            // Normal exit, no restart
            Ok(true)
        });
        #[cfg(feature = "instrument")]
        let fut = fut.instrument(tracing::trace_span!("igd tick fut"));
        fut.await
    }

    /////////////////////////////////////////////////////////////////////
    // Private Implementation

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn get_routed_local_ip_address(&self, address_type: IGDAddressType) -> Option<IpAddr> {
        let socket = match UdpSocket::bind(match address_type {
            IGDAddressType::IPV4 => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            IGDAddressType::IPV6 => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        }) {
            Ok(s) => s,
            Err(e) => {
                veilid_log!(self debug "failed to bind to unspecified address: {}", e);
                return None;
            }
        };

        // can be any routable ip address,
        // this is just to make the system routing table calculate the appropriate local ip address
        // using google's dns, but it wont actually send any packets to it
        socket
            .connect(match address_type {
                IGDAddressType::IPV4 => SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80),
                IGDAddressType::IPV6 => SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888)),
                    80,
                ),
            })
            .map_err(|e| {
                veilid_log!(self debug "failed to connect to dummy address: {}", e);
                e
            })
            .ok()?;

        Some(socket.local_addr().ok()?.ip())
    }

    /// Read the routed local IP from the cache; `None` if unprobed or no routable IP
    fn find_local_ip_inner(
        inner: &IGDManagerInner,
        address_type: IGDAddressType,
    ) -> Option<IpAddr> {
        inner.local_ip_addrs.get(&address_type).copied().flatten()
    }

    /// Read the IGD gateway from the cache; `None` if unprobed or no gateway
    fn find_gateway_inner(inner: &IGDManagerInner, local_ip: IpAddr) -> Option<Arc<Gateway>> {
        inner.gateways.get(&local_ip).cloned().flatten()
    }

    /// Read the external IP from the cache; `None` if unprobed or no external IP
    fn find_external_ip_inner(inner: &IGDManagerInner, local_ip: IpAddr) -> Option<IpAddr> {
        inner.external_ips.get(&local_ip).copied().flatten()
    }

    fn get_description(&self, protocol_type: IGDProtocolType, local_port: u16) -> String {
        format!(
            "{} map {} for port {}",
            self.registry.program_name(),
            protocol_type,
            local_port
        )
    }
}
