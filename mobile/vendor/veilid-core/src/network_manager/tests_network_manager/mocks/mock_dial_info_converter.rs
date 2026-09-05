use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct MockDialInfoConverter {}

impl DialInfoConverterResolver for MockDialInfoConverter {
    fn ptr_lookup(&self, _ip_addr: IpAddr) -> PinBoxFuture<'_, EyreResult<String>> {
        pin_dyn_future!(async move { Ok("fake_hostname".to_string()) })
    }

    fn to_socket_addrs(
        &self,
        _host: &str,
        default: SocketAddr,
    ) -> std::io::Result<std::vec::IntoIter<SocketAddr>> {
        Ok(vec![default].into_iter())
    }
}
