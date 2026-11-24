pub mod asset;
pub mod credentials;
pub mod generation;
pub mod studio;
pub mod worker;

pub const LABEL_MANAGED: &str = "app.kubernetes.io/managed-by";
pub const LABEL_NAME: &str = "app.kubernetes.io/name";
pub const LABEL_INSTANCE: &str = "app.kubernetes.io/instance";
pub const LABEL_COMPONENT: &str = "app.kubernetes.io/component";
pub const MANAGED_BY: &str = "freya-operator";

pub fn default_api_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-api".into(),
        "0.1.0".into(),
    )
}

pub fn default_ui_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-web".into(),
        "0.1.0".into(),
    )
}

pub fn default_mcp_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-mcp".into(),
        "0.1.0".into(),
    )
}

pub fn default_worker_image() -> (String, String) {
    (
        "harbor.dataknife.net/library/freya-diffusion-worker".into(),
        "0.1.0".into(),
    )
}

/// True when a worker URL is loopback, private, link-local, or cluster DNS.
/// Public hostnames keep certificate verification. Same rule as freya-api.
pub fn tls_skip_allowed(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host_is_local_or_private(host)
}

fn host_is_local_or_private(host: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if h.is_empty()
        || h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".cluster.local")
        || h.ends_with(".svc")
        || !h.contains('.')
    {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ipv4_local(ip),
        Ok(std::net::IpAddr::V6(ip)) => ipv6_local(ip),
        Err(_) => false,
    }
}

fn ipv4_local(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || a == 0
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
}

fn ipv6_local(ip: std::net::Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unicast_link_local() {
        return true;
    }
    let o = ip.octets();
    (o[0] & 0xfe) == 0xfc
}

#[cfg(test)]
mod tests {
    use super::tls_skip_allowed;

    #[test]
    fn public_https_does_not_skip_verify() {
        assert!(!tls_skip_allowed("https://vllm.dataknife.net"));
        assert!(tls_skip_allowed("https://192.168.3.125:8091"));
        assert!(tls_skip_allowed("https://127.0.0.1:8091"));
        assert!(tls_skip_allowed(
            "https://freya-gpu.freya.svc.cluster.local:8091"
        ));
        assert!(tls_skip_allowed("http://freya-worker:8091"));
    }
}
