use tracing::warn;

use super::validation::ValidationError;

pub const RESERVED_FDS: u64 = 64;
pub const DOCKER_DEFAULT_NOFILE: u64 = 1024;

#[cfg(unix)]
pub fn soft_nofile_limit() -> Option<u64> {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } == 0 {
        if lim.rlim_cur == libc::RLIM_INFINITY {
            return None;
        }
        return Some(lim.rlim_cur);
    }
    None
}

#[cfg(not(unix))]
pub fn soft_nofile_limit() -> Option<u64> {
    None
}

pub fn check_fd_budget(max_connections: usize, soft_limit: u64) -> Result<(), String> {
    let required = max_connections as u64 + RESERVED_FDS;
    if required > soft_limit {
        Err(format!(
            "max_connections ({max_connections}) + reserved FDs ({RESERVED_FDS}) exceeds soft RLIMIT_NOFILE ({soft_limit})"
        ))
    } else {
        Ok(())
    }
}

pub fn validate_nofile(max_connections: usize) -> Vec<ValidationError> {
    let Some(soft_limit) = soft_nofile_limit() else {
        return Vec::new();
    };
    match check_fd_budget(max_connections, soft_limit) {
        Ok(()) => Vec::new(),
        Err(_) => vec![ValidationError::MaxConnectionsExceedsNofile {
            max_connections,
            soft_limit,
            reserved: RESERVED_FDS,
        }],
    }
}

pub fn warn_if_default_nofile(max_connections: usize) {
    let Some(soft_limit) = soft_nofile_limit() else {
        return;
    };
    if soft_limit <= DOCKER_DEFAULT_NOFILE {
        warn!(
            soft_nofile = soft_limit,
            max_connections = max_connections,
            reserved_fds = RESERVED_FDS,
            "RLIMIT_NOFILE is at or below the Docker default ({}); the proxy needs headroom for \
             connections plus listener sockets, log files, and ACME renewal. Raise it (e.g. \
             docker-compose ulimits: nofile 8192, or systemd LimitNOFILE=8192) — see review #010",
            DOCKER_DEFAULT_NOFILE
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fd_budget_fits_when_max_connections_plus_reserved_within_limit() {
        assert!(check_fd_budget(800, 8192).is_ok());
        assert!(check_fd_budget(960, 1024).is_ok());
    }

    #[test]
    fn fd_budget_rejects_when_max_connections_plus_reserved_exceeds_limit() {
        assert!(check_fd_budget(1024, 1024).is_err());
        assert!(check_fd_budget(1023, 1024).is_err());
        assert!(check_fd_budget(8129, 8192).is_err());
    }

    #[test]
    fn fd_budget_boundary_exactly_at_limit_passes() {
        assert!(check_fd_budget(960, 1024).is_ok());
        assert!(check_fd_budget(961, 1024).is_err());
    }

    #[test]
    fn validate_nofile_rejects_absurd_max_connections_on_finite_limits() {
        if soft_nofile_limit().is_none() {
            return;
        }
        let errors = validate_nofile(usize::MAX / 2);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            ValidationError::MaxConnectionsExceedsNofile { .. }
        ));
    }

    #[test]
    fn validate_nofile_accepts_sane_max_connections_in_this_environment() {
        if soft_nofile_limit().is_none() {
            return;
        }
        let soft = soft_nofile_limit().unwrap();
        let sane = usize::try_from(soft.saturating_sub(RESERVED_FDS)).unwrap_or(1);
        assert!(validate_nofile(sane).is_empty());
    }
}