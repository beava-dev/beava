//! Transparent Huge Pages (THP) detection + self-opt-out.
//!
//! Why this exists:
//! - THP is a Linux feature that promotes 4 KB pages to 2 MB pages.
//! - With THP enabled, fork()'s copy-on-write (COW) granularity is 2 MB
//!   instead of 4 KB — modifying ONE byte during a snapshot copies a
//!   full 2 MB page. That's a 500× amplifier on COW memory overhead.
//! - Redis's well-known startup warning ("WARNING you have Transparent
//!   Huge Pages (THP) support enabled in your kernel") exists for the
//!   same reason — BGSAVE pays the same cost.
//!
//! What this module does (Linux only):
//! 1. Reads `/sys/kernel/mm/transparent_hugepage/enabled` and logs a
//!    structured WARN if the kernel default is `[always]`.
//! 2. Calls `prctl(PR_SET_THP_DISABLE, 1)` to opt THIS PROCESS out of
//!    THP regardless of the system-wide setting. This is the
//!    self-protection Redis-clone projects also do (KeyDB,
//!    Dragonfly, etc.).
//!
//! Non-Linux platforms (macOS, *BSD) have no THP — this module is a
//! no-op there.

#[cfg(target_os = "linux")]
const THP_ENABLED_PATH: &str = "/sys/kernel/mm/transparent_hugepage/enabled";

/// Detect kernel THP setting + opt this process out. Safe to call
/// multiple times; idempotent.
pub fn detect_and_opt_out() {
    #[cfg(target_os = "linux")]
    {
        // 1. Detect system-wide setting for operator awareness.
        match std::fs::read_to_string(THP_ENABLED_PATH) {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.contains("[always]") {
                    tracing::warn!(
                        target: "beava.thp",
                        kind = "thp.system_always_enabled",
                        setting = %trimmed,
                        path = THP_ENABLED_PATH,
                        "Transparent Huge Pages (THP) is set to `always` system-wide. \
                         beava is opting this process out via prctl, but operators should \
                         set THP to `madvise` or `never` system-wide for best fork+COW \
                         performance: \
                         `echo madvise > /sys/kernel/mm/transparent_hugepage/enabled` \
                         (or kernel boot param `transparent_hugepage=madvise`). \
                         See: https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/latency/#latency-induced-by-transparent-huge-pages"
                    );
                } else {
                    tracing::debug!(
                        target: "beava.thp",
                        kind = "thp.system_setting",
                        setting = %trimmed,
                        "system THP setting (recommended: `madvise` or `never`)"
                    );
                }
            }
            Err(e) => {
                // /sys/kernel/mm may not exist on all kernels / containers;
                // not a problem — just log at debug.
                tracing::debug!(
                    target: "beava.thp",
                    kind = "thp.sys_unreadable",
                    error = %e,
                    "could not read THP setting (non-Linux kernel or sandboxed /sys)"
                );
            }
        }

        // 2. Self-opt-out for this process. PR_SET_THP_DISABLE = 41.
        // SAFETY: prctl is async-signal-safe and has no parameter aliasing
        // concerns; the only effect of failure is the process stays
        // subject to system THP (we log the failure and move on).
        let ret = unsafe { libc::prctl(libc::PR_SET_THP_DISABLE, 1u64, 0u64, 0u64, 0u64) };
        if ret == 0 {
            tracing::debug!(
                target: "beava.thp",
                kind = "thp.process_opt_out_ok",
                "process opted out of THP via prctl(PR_SET_THP_DISABLE) — \
                 fork()+COW page granularity now 4 KB regardless of system setting"
            );
        } else {
            let err = std::io::Error::last_os_error();
            tracing::warn!(
                target: "beava.thp",
                kind = "thp.process_opt_out_failed",
                error = %err,
                "prctl(PR_SET_THP_DISABLE) failed; this process may still pay \
                 2 MB-page COW cost during snapshots if system THP is enabled"
            );
        }
    }

    // macOS / *BSD: no THP, no work to do.
    #[cfg(not(target_os = "linux"))]
    {
        tracing::debug!(
            target: "beava.thp",
            kind = "thp.not_applicable",
            "THP check skipped (non-Linux platform)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_and_opt_out_does_not_panic() {
        // Idempotent + side-effect-only — just verify the call doesn't
        // explode on either Linux or macOS.
        detect_and_opt_out();
        detect_and_opt_out();
    }
}
