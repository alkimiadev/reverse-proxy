#[cfg(test)]
#[derive(Default)]
struct KvVisitor {
    pairs: Vec<(String, String)>,
}

#[cfg(test)]
impl KvVisitor {
    fn format(&self) -> String {
        let parts: Vec<String> = self
            .pairs
            .iter()
            .map(|(k, v)| {
                if v.contains(' ') || v.contains('"') {
                    format!(r#"{}="{}""#, k, v.replace('"', "\\\""))
                } else {
                    format!("{}={}", k, v)
                }
            })
            .collect();
        parts.join(" ")
    }
}

#[cfg(test)]
impl tracing::field::Visit for KvVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.pairs
            .push((field.name().to_string(), format!("{:?}", value)));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.pairs
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.pairs
            .push((field.name().to_string(), value.to_string()));
    }
}

#[macro_export]
macro_rules! log_request {
    ($client_ip:expr, $host:expr, $method:expr, $path:expr, $status:expr, $upstream:expr, $duration_ms:expr) => {
        tracing::info!(
            prefix = "REQUEST",
            client_ip = %$client_ip,
            host = %$host,
            method = %$method,
            path = %$path,
            status = %$status,
            upstream = %$upstream,
            duration_ms = %$duration_ms,
        )
    };
}

#[macro_export]
macro_rules! log_upstream_error {
    ($host:expr, $upstream:expr, $error:expr) => {
        tracing::warn!(
            prefix = "UPSTREAM_ERROR",
            host = %$host,
            upstream = %$upstream,
            error = %$error,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_visitor_formats_simple_key_value() {
        let mut visitor = KvVisitor::default();
        visitor
            .pairs
            .push(("client_ip".to_string(), "203.0.113.50".to_string()));
        visitor
            .pairs
            .push(("status".to_string(), "200".to_string()));
        assert_eq!(visitor.format(), "client_ip=203.0.113.50 status=200");
    }

    #[test]
    fn kv_visitor_quotes_values_with_spaces() {
        let mut visitor = KvVisitor::default();
        visitor
            .pairs
            .push(("error".to_string(), "connection refused".to_string()));
        assert_eq!(visitor.format(), r#"error="connection refused""#);
    }

    #[test]
    fn kv_visitor_escapes_quotes_in_values() {
        let mut visitor = KvVisitor::default();
        visitor
            .pairs
            .push(("error".to_string(), r#"connection "timeout""#.to_string()));
        let formatted = visitor.format();
        assert!(
            formatted.contains(r#"connection \"timeout\""#),
            "got: {}",
            formatted
        );
    }

    #[test]
    fn kv_visitor_handles_numeric_values() {
        let mut visitor = KvVisitor::default();
        visitor
            .pairs
            .push(("status".to_string(), "200".to_string()));
        visitor
            .pairs
            .push(("duration_ms".to_string(), "45".to_string()));
        let formatted = visitor.format();
        assert!(formatted.contains("status=200"));
        assert!(formatted.contains("duration_ms=45"));
    }

    #[test]
    fn kv_visitor_handles_empty() {
        let visitor = KvVisitor::default();
        assert_eq!(visitor.format(), "");
    }

    #[test]
    fn log_macros_compile() {
        use tracing_subscriber::layer::SubscriberExt;

        let subscriber = tracing_subscriber::registry().with(tracing_subscriber::fmt::layer());
        let _guard = tracing::subscriber::set_default(subscriber);

        log_request!(
            "203.0.113.50",
            "git.alk.dev",
            "GET",
            "/user/repo",
            200,
            "127.0.0.1:3000",
            45u64
        );
        log_upstream_error!("git.alk.dev", "127.0.0.1:3000", "connection refused");
    }
}
