use super::*;
use lazy_static::*;

lazy_static! {
    static ref BAD_PORTS: BTreeSet<u16> = BTreeSet::from([
        1,    // tcpmux
        7,    // echo
        9,    // discard
        11,   // systat
        13,   // daytime
        15,   // netstat
        17,   // qotd
        19,   // chargen
        20,   // ftp data
        21,   // ftp access
        22,   // ssh
        23,   // telnet
        25,   // smtp
        37,   // time
        42,   // name
        43,   // nicname
        53,   // domain
        77,   // priv-rjs
        79,   // finger
        87,   // ttylink
        95,   // supdup
        101,  // hostriame
        102,  // iso-tsap
        103,  // gppitnp
        104,  // acr-nema
        109,  // pop2
        110,  // pop3
        111,  // sunrpc
        113,  // auth
        115,  // sftp
        117,  // uucp-path
        119,  // nntp
        123,  // NTP
        135,  // loc-srv /epmap
        139,  // netbios
        143,  // imap2
        179,  // BGP
        389,  // ldap
        427,  // SLP (Also used by Apple Filing Protocol)
        465,  // smtp+ssl
        512,  // print / exec
        513,  // login
        514,  // shell
        515,  // printer
        526,  // tempo
        530,  // courier
        531,  // chat
        532,  // netnews
        540,  // uucp
        548,  // AFP (Apple Filing Protocol)
        556,  // remotefs
        563,  // nntp+ssl
        587,  // smtp (rfc6409)
        601,  // syslog-conn (rfc3195)
        636,  // ldap+ssl
        993,  // ldap+ssl
        995,  // pop3+ssl
        2049, // nfs
        3659, // apple-sasl / PasswordServer
        4045, // lockd
        6000, // X11
        6665, // Alternate IRC [Apple addition]
        6666, // Alternate IRC [Apple addition]
        6667, // Standard IRC [Apple addition]
        6668, // Alternate IRC [Apple addition]
        6669, // Alternate IRC [Apple addition]
        6697, // IRC + TLS
    ]);
}

/// Given a 'listen' specifier string from the config:
/// a port, a set of ip addresses to bind to, and a
/// bool specifying if multiple ports should be tried
pub(super) struct NetworkBindSet {
    pub port: u16,
    pub addrs: Vec<IpAddr>,
    pub search: bool,
}

impl FromStr for NetworkBindSet {
    type Err = EyreReport;

    fn from_str(listen_address: &str) -> Result<Self, Self::Err> {
        if listen_address.is_empty() {
            // If listen address is empty, start with port 5150 and iterate
            let ip_addrs = available_unspecified_addresses();
            Ok(NetworkBindSet {
                port: 5150,
                addrs: ip_addrs,
                search: true,
            })
        } else {
            // If no address is specified, but the port is, use ipv4 and ipv6 unspecified
            // If the address is specified, only use the specified port and fail otherwise
            let sockaddrs =
                listen_address_to_socket_addrs(listen_address).map_err(|e| eyre!("{}", e))?;
            if sockaddrs.is_empty() {
                bail!("No valid listen address: {}", listen_address);
            }
            let port = sockaddrs[0].port();
            if port == 0 {
                Ok(NetworkBindSet {
                    port: 5150,
                    addrs: sockaddrs.iter().map(|s| s.ip()).collect(),
                    search: true,
                })
            } else {
                Ok(NetworkBindSet {
                    port,
                    addrs: sockaddrs.iter().map(|s| s.ip()).collect(),
                    search: false,
                })
            }
        }
    }
}

impl NativeNetwork {
    /////////////////////////////////////////////////////

    // Add local dial info to preferred local address table
    pub(super) fn set_preferred_local_address(inner: &mut NetworkInner, pa: PeerAddress) {
        let key = pa.transport_type();
        let sa = pa.socket_addr();
        // let unspec_sa = match sa {
        //     SocketAddr::V4(a) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, a.port())),
        //     SocketAddr::V6(a) => {
        //         SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, a.port(), 0, 0))
        //     }
        // };
        inner.preferred_local_addresses.entry(key).or_insert(sa);
    }

    /////////////////////////////////////////////////////

    /// Parse a listen address into a bind set, restricting bind addresses to the
    /// address types enabled in config. Default/port-only/hostname addresses keep
    /// only their enabled families; an explicit single-family address of a disabled
    /// type filters to nothing and is reported as a conflict.
    fn make_bind_set(&self, listen_address: &str) -> EyreResult<NetworkBindSet> {
        let mut bind_set = NetworkBindSet::from_str(listen_address)?;
        let allowed = configured_address_type_set(&self.config());

        bind_set.addrs.retain(|ip| {
            let at = if ip.is_ipv4() {
                AddressType::IPV4
            } else {
                AddressType::IPV6
            };
            allowed.contains(at)
        });
        if bind_set.addrs.is_empty() {
            bail!(
                "listen address '{}' has no address family enabled in network.address_types",
                listen_address
            );
        }
        Ok(bind_set)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn bind_udp_protocol_handlers(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self trace "UDP: binding protocol handlers");
        let listen_address = self.config().network.protocol.udp.listen_address.clone();

        // Get the binding parameters from the user-specified listen address
        let bind_set = self.make_bind_set(&listen_address)?;

        // Now create udp inbound sockets for whatever interfaces we're listening on
        if bind_set.search {
            veilid_log!(self info
                "UDP: searching for free port starting with {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        } else {
            veilid_log!(self info
                "UDP: binding protocol handlers at port {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        }

        if !self.create_inbound_udp_protocol_handlers(bind_set)? {
            return Ok(StartupDisposition::BindRetry);
        };

        if !self.create_outbound_udp_protocol_handlers()? {
            return Ok(StartupDisposition::BindRetry);
        }

        // Now create tasks for udp listeners
        self.create_udp_listener_tasks()?;

        Ok(StartupDisposition::Success)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn add_udp_interface_dial_info(
        &self,
        config: Arc<VeilidConfig>,
        bound_addresses: &[SocketAddr],
        network_state: &mut NetworkState,
    ) -> EyreResult<()> {
        // Add the statically configured public addresses to the set to create dialinfo for
        // and note which ones were static in their own set
        let public_address = config.network.protocol.udp.public_address.clone();
        if let Some(public_address) = public_address.as_ref() {
            let static_addrs = public_address
                .to_socket_addrs()
                .wrap_err("failed to resolve udp address")?;

            for static_addr in static_addrs {
                let did = DialInfoDetail {
                    dial_info: DialInfo::udp_from_socketaddr(static_addr),
                    class: DialInfoClass::Direct,
                };

                if let Err(e) = network_state.add_static_dial_info_detail(did) {
                    veilid_log!(self error "Failed to add udp static dial info detail: {}", e);
                }
            }
        };

        // Add all the bound socket addresses to the set to create dialinfo for
        let mut interface_addrs = BTreeSet::new();
        for socket_addr in bound_addresses.iter().copied() {
            for idi_addr in network_state
                .translate_unspecified_socket_addr(socket_addr)
                .into_iter()
            {
                interface_addrs.insert(idi_addr);
            }
        }
        for interface_addr in interface_addrs {
            let did = DialInfoDetail {
                dial_info: DialInfo::udp_from_socketaddr(interface_addr),
                class: DialInfoClass::Direct,
            };

            if let Err(e) = network_state.add_interface_dial_info_detail(did) {
                veilid_log!(self error "Failed to add udp interface dial info detail: {}", e);
            }
        }

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn start_ws_listeners(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self trace "WS: binding protocol handlers");
        let listen_address = self.config().network.protocol.ws.listen_address.clone();

        // Get the binding parameters from the user-specified listen address
        let bind_set = self.make_bind_set(&listen_address)?;

        if bind_set.search {
            veilid_log!(self info
                "WS: searching for free port starting with {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        } else {
            veilid_log!(self info
                "WS: binding protocol handlers at port {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        }
        if !self.start_tcp_listener(
            bind_set,
            false,
            ProtocolType::WS,
            Box::new(|r, t| Box::new(WebsocketProtocolHandler::new(r, t))),
        )? {
            return Ok(StartupDisposition::BindRetry);
        }

        Ok(StartupDisposition::Success)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn add_ws_interface_dial_info(
        &self,
        config: Arc<VeilidConfig>,
        bound_addresses: &[SocketAddr],
        network_state: &mut NetworkState,
    ) -> EyreResult<()> {
        // Add all the bound socket addresses to the set to create dialinfo for
        let mut all_bound_socket_addrs = BTreeSet::new();
        for addr in bound_addresses.iter().copied() {
            for idi_addr in network_state
                .translate_unspecified_socket_addr(addr)
                .into_iter()
            {
                all_bound_socket_addrs.insert(idi_addr);
            }
        }

        // Add the statically configured public addresses to the set to create dialinfo for
        // and note which ones were static in their own set
        let url = config.network.protocol.ws.url.clone();

        if let Some(url) = url.as_ref() {
            let mut split_url = SplitUrl::from_str(url).wrap_err("couldn't split url")?;
            if !split_url.scheme.eq_ignore_ascii_case("ws") {
                bail!("WS URL must use 'ws://' scheme");
            }
            "ws".clone_into(&mut split_url.scheme);

            // Resolve static public hostnames
            let static_addrs = split_url
                .host_port(80)
                .to_socket_addrs()
                .wrap_err("failed to resolve ws url")?
                .collect::<BTreeSet<_>>();

            for static_addr in static_addrs.iter().copied() {
                let did = DialInfoDetail {
                    dial_info: DialInfo::try_ws(
                        SocketAddress::from_socket_addr(static_addr),
                        url.clone(),
                    )
                    .wrap_err("try_ws failed")?,
                    class: DialInfoClass::Direct,
                };

                if let Err(e) = network_state.add_static_dial_info_detail(did) {
                    veilid_log!(self error "Failed to add ws static dial info detail: {}", e);
                }
            }
        }

        let path = config.network.protocol.ws.path.clone();
        for socket_addr in all_bound_socket_addrs {
            let addr_url = format!("ws://{}/{}", socket_addr, path);
            let did = DialInfoDetail {
                dial_info: DialInfo::try_ws(SocketAddress::from_socket_addr(socket_addr), addr_url)
                    .wrap_err("try_ws failed")?,
                class: DialInfoClass::Direct,
            };

            if let Err(e) = network_state.add_interface_dial_info_detail(did) {
                veilid_log!(self error "Failed to add ws interface dial info detail: {}", e);
            }
        }

        Ok(())
    }

    #[cfg(feature = "enable-protocol-wss")]
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn start_wss_listeners(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self trace "WSS: binding protocol handlers");

        let listen_address = self.config().network.protocol.wss.listen_address.clone();

        // Get the binding parameters from the user-specified listen address
        let bind_set = self.make_bind_set(&listen_address)?;

        if bind_set.search {
            veilid_log!(self info
                "WSS: searching for free port starting with {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        } else {
            veilid_log!(self info
                "WSS: binding protocol handlers at port {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        }

        if !self.start_tcp_listener(
            bind_set,
            true,
            ProtocolType::WSS,
            Box::new(|r, t| Box::new(WebsocketProtocolHandler::new(r, t))),
        )? {
            return Ok(StartupDisposition::BindRetry);
        }

        Ok(StartupDisposition::Success)
    }

    #[cfg(feature = "enable-protocol-wss")]
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn add_wss_interface_dial_info(
        &self,
        config: Arc<VeilidConfig>,
        bound_addresses: &[SocketAddr],
        network_state: &mut NetworkState,
    ) -> EyreResult<()> {
        // Add all the bound socket addresses to the set to create dialinfo for
        let mut all_bound_socket_addrs = BTreeSet::new();
        for addr in bound_addresses.iter().copied() {
            for idi_addr in network_state
                .translate_unspecified_socket_addr(addr)
                .into_iter()
            {
                all_bound_socket_addrs.insert(idi_addr);
            }
        }

        // Add the statically configured public addresses to the set to create dialinfo for
        // and note which ones were static in their own set
        let url = config.network.protocol.wss.url.clone();

        if let Some(url) = url.as_ref() {
            let mut split_url = SplitUrl::from_str(url).wrap_err("couldn't split url")?;
            if !split_url.scheme.eq_ignore_ascii_case("wss") {
                bail!("WSS URL must use 'wss://' scheme");
            }
            "wss".clone_into(&mut split_url.scheme);

            // Resolve static public hostnames
            let static_addrs = split_url
                .host_port(443)
                .to_socket_addrs()
                .wrap_err("failed to resolve wss url")?
                .collect::<BTreeSet<_>>();

            for static_addr in static_addrs.iter().copied() {
                let did = DialInfoDetail {
                    dial_info: DialInfo::try_wss(
                        SocketAddress::from_socket_addr(static_addr),
                        url.clone(),
                    )
                    .wrap_err("try_wss failed")?,
                    class: DialInfoClass::Direct,
                };

                if let Err(e) = network_state.add_static_dial_info_detail(did) {
                    veilid_log!(self error "Failed to add wss static dial info detail: {}", e);
                }
            }
        } else {
            bail!("WSS URL must be specified due to TLS requirements");
        };

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn start_tcp_listeners(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self trace "TCP: binding protocol handlers");

        let listen_address = self.config().network.protocol.tcp.listen_address.clone();

        // Get the binding parameters from the user-specified listen address
        let bind_set = self.make_bind_set(&listen_address)?;

        if bind_set.search {
            veilid_log!(self info
                "TCP: searching for free port starting with {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        } else {
            veilid_log!(self info
                "TCP: binding protocol handlers at port {} on {:?}",
                bind_set.port, bind_set.addrs
            );
        }
        if !self.start_tcp_listener(
            bind_set,
            false,
            ProtocolType::TCP,
            Box::new(|c, _| Box::new(RawTcpProtocolHandler::new(c))),
        )? {
            return Ok(StartupDisposition::BindRetry);
        }

        Ok(StartupDisposition::Success)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn add_tcp_interface_dial_info(
        &self,
        config: Arc<VeilidConfig>,
        bound_addresses: &[SocketAddr],
        network_state: &mut NetworkState,
    ) -> EyreResult<()> {
        // Add the statically configured public addresses to the set to create dialinfo for
        // and note which ones were static in their own set
        let public_address = config.network.protocol.tcp.public_address.clone();
        if let Some(public_address) = public_address.as_ref() {
            let static_addrs = public_address
                .to_socket_addrs()
                .wrap_err("failed to resolve tcp address")?;

            for static_addr in static_addrs {
                let did = DialInfoDetail {
                    dial_info: DialInfo::tcp_from_socketaddr(static_addr),
                    class: DialInfoClass::Direct,
                };

                if let Err(e) = network_state.add_static_dial_info_detail(did) {
                    veilid_log!(self error "Failed to add tcp static dial info detail: {}", e);
                }
            }
        };

        // Add all the bound socket addresses to the set to create dialinfo for
        let mut interface_addrs = BTreeSet::new();
        for socket_addr in bound_addresses.iter().copied() {
            for idi_addr in network_state
                .translate_unspecified_socket_addr(socket_addr)
                .into_iter()
            {
                interface_addrs.insert(idi_addr);
            }
        }
        for interface_addr in interface_addrs {
            let did = DialInfoDetail {
                dial_info: DialInfo::tcp_from_socketaddr(interface_addr),
                class: DialInfoClass::Direct,
            };

            if let Err(e) = network_state.add_interface_dial_info_detail(did) {
                veilid_log!(self error "Failed to add tcp interface dial info detail: {}", e);
            }
        }

        Ok(())
    }
}
