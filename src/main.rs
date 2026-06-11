mod admin;
mod config;
mod health;
mod logging;
mod proxy;
mod rate_limit;
mod shutdown;
mod tls;

fn main() {
    tracing::info!("reverse-proxy starting");
}
