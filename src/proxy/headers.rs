use axum::http::{HeaderMap, HeaderName, HeaderValue};
use std::net::SocketAddr;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

pub fn remove_hop_by_hop(headers: &mut HeaderMap) {
    for &name in HOP_BY_HOP {
        headers.remove(name);
    }
}

pub fn inject_proxy_headers(headers: &mut HeaderMap, remote_addr: SocketAddr, is_https: bool) {
    let ip_str = remote_addr.ip().to_string();
    let ip_value =
        HeaderValue::from_str(&ip_str).unwrap_or_else(|_| HeaderValue::from_static("0.0.0.0"));

    headers.insert(HeaderName::from_static("x-real-ip"), ip_value.clone());

    headers.insert(HeaderName::from_static("x-forwarded-for"), ip_value);

    let proto_value = if is_https {
        HeaderValue::from_static("https")
    } else {
        HeaderValue::from_static("http")
    };
    headers.insert(HeaderName::from_static("x-forwarded-proto"), proto_value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_headers_with_hop_by_hop() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("connection", HeaderValue::from_static("keep-alive"));
        h.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        h.insert("proxy-authorization", HeaderValue::from_static("Basic abc"));
        h.insert(
            "proxy-authenticate",
            HeaderValue::from_static("Basic realm=x"),
        );
        h.insert("te", HeaderValue::from_static("trailers"));
        h.insert("trailers", HeaderValue::from_static("chunked"));
        h.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        h.insert("upgrade", HeaderValue::from_static("websocket"));
        h.insert("content-type", HeaderValue::from_static("text/html"));
        h.insert("accept", HeaderValue::from_static("*/*"));
        h
    }

    #[test]
    fn remove_hop_by_hop_removes_all_listed_headers() {
        let mut h = make_headers_with_hop_by_hop();
        remove_hop_by_hop(&mut h);
        assert!(h.get("connection").is_none());
        assert!(h.get("keep-alive").is_none());
        assert!(h.get("proxy-authorization").is_none());
        assert!(h.get("proxy-authenticate").is_none());
        assert!(h.get("te").is_none());
        assert!(h.get("trailers").is_none());
        assert!(h.get("transfer-encoding").is_none());
        assert!(h.get("upgrade").is_none());
    }

    #[test]
    fn remove_hop_by_hop_preserves_other_headers() {
        let mut h = make_headers_with_hop_by_hop();
        remove_hop_by_hop(&mut h);
        assert_eq!(h.get("content-type").unwrap(), "text/html");
        assert_eq!(h.get("accept").unwrap(), "*/*");
    }

    #[test]
    fn inject_proxy_headers_sets_x_real_ip() {
        let mut h = HeaderMap::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 12345);
        inject_proxy_headers(&mut h, addr, true);
        assert_eq!(h.get("x-real-ip").unwrap(), "192.168.1.1");
    }

    #[test]
    fn inject_proxy_headers_replaces_x_forwarded_for() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("10.0.0.1, 10.0.0.2"),
        );
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 12345);
        inject_proxy_headers(&mut h, addr, true);
        assert_eq!(h.get("x-forwarded-for").unwrap(), "192.168.1.1");
    }

    #[test]
    fn inject_proxy_headers_sets_x_forwarded_proto_https() {
        let mut h = HeaderMap::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443);
        inject_proxy_headers(&mut h, addr, true);
        assert_eq!(h.get("x-forwarded-proto").unwrap(), "https");
    }

    #[test]
    fn inject_proxy_headers_sets_x_forwarded_proto_http() {
        let mut h = HeaderMap::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80);
        inject_proxy_headers(&mut h, addr, false);
        assert_eq!(h.get("x-forwarded-proto").unwrap(), "http");
    }

    #[test]
    fn inject_proxy_headers_preserves_host() {
        let mut h = HeaderMap::new();
        h.insert("host", HeaderValue::from_static("example.com"));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443);
        inject_proxy_headers(&mut h, addr, true);
        assert_eq!(h.get("host").unwrap(), "example.com");
    }

    #[test]
    fn remove_hop_by_hop_empty_headers() {
        let mut h = HeaderMap::new();
        remove_hop_by_hop(&mut h);
        assert!(h.is_empty());
    }
}
