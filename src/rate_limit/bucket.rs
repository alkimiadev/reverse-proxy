use std::net::{IpAddr, Ipv6Addr};
use std::time::Instant;

pub struct TokenBucket {
    pub tokens: f64,
    pub last_refill: Instant,
    pub rate: f64,
    pub max: u32,
    pub last_access: Instant,
}

impl TokenBucket {
    pub fn new(rate: f64, max: u32) -> Self {
        Self {
            tokens: max as f64,
            last_refill: Instant::now(),
            rate,
            max,
            last_access: Instant::now(),
        }
    }

    pub fn try_consume(&mut self, rate: f64, max: u32) -> bool {
        self.refill(rate, max);
        self.last_access = Instant::now();

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self, rate: f64, max: u32) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_millis() as f64;
        let tokens_to_add = (elapsed * rate) / 1000.0;
        self.tokens = (self.tokens + tokens_to_add).min(max as f64);
        self.last_refill = now;

        if max < self.max {
            self.tokens = self.tokens.min(max as f64);
        }
        self.max = max;
        self.rate = rate;
    }
}

pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            let normalized = Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                0,
                0,
                0,
                0,
            );
            IpAddr::V6(normalized)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn token_bucket_starts_full() {
        let bucket = TokenBucket::new(10.0, 20);
        assert_eq!(bucket.tokens, 20.0);
    }

    #[test]
    fn token_bucket_consume_within_burst() {
        let mut bucket = TokenBucket::new(10.0, 20);
        for _ in 0..20 {
            assert!(bucket.try_consume(10.0, 20));
        }
    }

    #[test]
    fn token_bucket_reject_when_empty() {
        let mut bucket = TokenBucket::new(10.0, 5);
        for _ in 0..5 {
            assert!(bucket.try_consume(10.0, 5));
        }
        assert!(!bucket.try_consume(10.0, 5));
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let mut bucket = TokenBucket::new(1000.0, 5);
        for _ in 0..5 {
            assert!(bucket.try_consume(1000.0, 5));
        }
        assert!(!bucket.try_consume(1000.0, 5));

        thread::sleep(Duration::from_millis(10));
        assert!(bucket.try_consume(1000.0, 5));
    }

    #[test]
    fn token_bucket_caps_at_max() {
        let mut bucket = TokenBucket::new(10.0, 5);
        for _ in 0..5 {
            assert!(bucket.try_consume(10.0, 5));
        }
        thread::sleep(Duration::from_millis(500));
        bucket.try_consume(10.0, 5);
        assert!(bucket.tokens <= 5.5);
    }

    #[test]
    fn token_bucket_config_reload_caps_tokens() {
        let mut bucket = TokenBucket::new(10.0, 20);
        assert!(bucket.try_consume(10.0, 20));

        let new_max = 10;
        bucket.refill(10.0, new_max);
        assert!(bucket.tokens <= new_max as f64 + 1.0);
    }

    #[test]
    fn token_bucket_config_reload_adopts_new_rate() {
        let mut bucket = TokenBucket::new(10.0, 5);
        for _ in 0..5 {
            assert!(bucket.try_consume(10.0, 5));
        }
        assert!(!bucket.try_consume(10.0, 5));

        thread::sleep(Duration::from_millis(200));
        assert!(bucket.try_consume(50.0, 5));
    }

    #[test]
    fn normalize_ipv4_unchanged() {
        let ip = IpAddr::from(Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(normalize_ip(ip), ip);
    }

    #[test]
    fn normalize_ipv6_truncates_to_64_prefix() {
        let ip = IpAddr::from(Ipv6Addr::new(
            0x2001, 0x0db8, 0x85a3, 0x0001, 0x1234, 0x5678, 0x9abc, 0xdef0,
        ));
        let normalized = normalize_ip(ip);
        let expected = IpAddr::from(Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0001, 0, 0, 0, 0));
        assert_eq!(normalized, expected);
    }

    #[test]
    fn normalize_ipv6_already_64_prefix() {
        let ip = IpAddr::from(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0));
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, ip);
    }

    #[test]
    fn normalize_ipv6_link_local() {
        let ip = IpAddr::from(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0x1234, 0x5678, 0x9abc, 0xdef0,
        ));
        let normalized = normalize_ip(ip);
        let expected = IpAddr::from(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0));
        assert_eq!(normalized, expected);
    }

    #[test]
    fn normalize_loopback_ipv6() {
        let ip = IpAddr::from(Ipv6Addr::LOCALHOST);
        let normalized = normalize_ip(ip);
        let expected = IpAddr::from(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0));
        assert_eq!(normalized, expected);
    }
}
