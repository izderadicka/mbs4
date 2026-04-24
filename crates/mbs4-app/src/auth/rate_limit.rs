use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

const MAX_ATTEMPTS: usize = 10;
const WINDOW: Duration = Duration::from_secs(60);
// Prune the map when it grows beyond this to bound memory usage.
const PRUNE_THRESHOLD: usize = 10_000;

#[derive(Clone)]
pub struct LoginRateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the request is allowed, `false` if the IP is over the limit.
    pub async fn check_and_record(&self, ip: IpAddr) -> bool {
        let mut map = self.inner.lock().await;
        let now = Instant::now();
        let window_start = now.checked_sub(WINDOW).unwrap_or(now);

        let allowed = {
            let attempts = map.entry(ip).or_default();
            attempts.retain(|&t| t > window_start);
            if attempts.len() >= MAX_ATTEMPTS {
                false
            } else {
                attempts.push(now);
                true
            }
        };

        if map.len() > PRUNE_THRESHOLD {
            map.retain(|_, attempts| attempts.iter().any(|&t| t > window_start));
        }

        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn allows_up_to_limit() {
        let limiter = LoginRateLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        for _ in 0..MAX_ATTEMPTS {
            assert!(limiter.check_and_record(ip).await);
        }
        assert!(!limiter.check_and_record(ip).await);
    }

    #[tokio::test]
    async fn different_ips_are_independent() {
        let limiter = LoginRateLimiter::new();
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let ip2 = IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8));
        for _ in 0..MAX_ATTEMPTS {
            limiter.check_and_record(ip1).await;
        }
        assert!(!limiter.check_and_record(ip1).await);
        assert!(limiter.check_and_record(ip2).await);
    }
}
