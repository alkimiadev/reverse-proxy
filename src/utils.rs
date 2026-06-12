pub fn strip_port_from_host(host: &str) -> &str {
    if host.starts_with('[') {
        if let Some(bracket_end) = host.find(']') {
            &host[..bracket_end + 1]
        } else {
            host
        }
    } else if let Some(colon_pos) = host.rfind(':') {
        &host[..colon_pos]
    } else {
        host
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_port_plain() {
        assert_eq!(strip_port_from_host("example.com"), "example.com");
    }

    #[test]
    fn strip_port_with_port() {
        assert_eq!(strip_port_from_host("example.com:8080"), "example.com");
    }

    #[test]
    fn strip_port_ipv6_bare() {
        assert_eq!(strip_port_from_host("[::1]"), "[::1]");
    }

    #[test]
    fn strip_port_ipv6_with_port() {
        assert_eq!(strip_port_from_host("[::1]:8080"), "[::1]");
    }

    #[test]
    fn strip_port_ipv6_with_long_address() {
        assert_eq!(strip_port_from_host("[2001:db8::1]:8080"), "[2001:db8::1]");
    }

    #[test]
    fn strip_port_ipv4_with_port() {
        assert_eq!(strip_port_from_host("192.168.1.1:8080"), "192.168.1.1");
    }
}
