use serde::{Deserialize, Serialize};

use crate::error::RemoteStorageError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCheckResult {
    pub fits: bool,
    pub current_usage_bytes: u64,
    pub requested_bytes: u64,
    pub storage_limit_bytes: Option<u64>,
    pub projected_usage_bytes: u64,
    pub remaining_bytes_after: Option<u64>,
}

/// Evaluates whether an upload of `requested_bytes` will fit within the user's storage limit.
pub fn evaluate_quota(
    current_usage_bytes: u64,
    requested_bytes: u64,
    storage_limit_bytes: Option<u64>,
) -> QuotaCheckResult {
    let projected_usage_bytes = current_usage_bytes.saturating_add(requested_bytes);

    match storage_limit_bytes {
        Some(limit) => {
            let fits = projected_usage_bytes <= limit;
            let remaining_bytes_after = if fits {
                Some(limit.saturating_sub(projected_usage_bytes))
            } else {
                Some(0)
            };

            QuotaCheckResult {
                fits,
                current_usage_bytes,
                requested_bytes,
                storage_limit_bytes: Some(limit),
                projected_usage_bytes,
                remaining_bytes_after,
            }
        }
        None => QuotaCheckResult {
            fits: true,
            current_usage_bytes,
            requested_bytes,
            storage_limit_bytes: None,
            projected_usage_bytes,
            remaining_bytes_after: None,
        },
    }
}

/// Verifies that `requested_bytes` fits within `storage_limit_bytes`, returning `Err(RemoteStorageError::QuotaExceeded)`
/// if it exceeds the limit.
pub fn ensure_within_quota(
    current_usage_bytes: u64,
    requested_bytes: u64,
    storage_limit_bytes: Option<u64>,
) -> Result<(), RemoteStorageError> {
    let check = evaluate_quota(current_usage_bytes, requested_bytes, storage_limit_bytes);
    if !check.fits {
        let available_bytes = storage_limit_bytes
            .map(|l| l.saturating_sub(current_usage_bytes))
            .unwrap_or(0);
        return Err(RemoteStorageError::QuotaExceeded {
            required_bytes: requested_bytes,
            available_bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlimited_quota() {
        let result = evaluate_quota(1_000_000, 500_000, None);
        assert!(result.fits);
        assert_eq!(result.projected_usage_bytes, 1_500_000);
        assert_eq!(result.remaining_bytes_after, None);
        assert!(ensure_within_quota(1_000_000, 500_000, None).is_ok());
    }

    #[test]
    fn test_within_quota() {
        let limit = 10_000_000;
        let result = evaluate_quota(4_000_000, 5_000_000, Some(limit));
        assert!(result.fits);
        assert_eq!(result.projected_usage_bytes, 9_000_000);
        assert_eq!(result.remaining_bytes_after, Some(1_000_000));
        assert!(ensure_within_quota(4_000_000, 5_000_000, Some(limit)).is_ok());
    }

    #[test]
    fn test_exceeds_quota() {
        let limit = 10_000_000;
        let result = evaluate_quota(8_000_000, 3_000_000, Some(limit));
        assert!(!result.fits);
        assert_eq!(result.projected_usage_bytes, 11_000_000);
        assert_eq!(result.remaining_bytes_after, Some(0));

        let err = ensure_within_quota(8_000_000, 3_000_000, Some(limit));
        match err {
            Err(RemoteStorageError::QuotaExceeded {
                required_bytes,
                available_bytes,
            }) => {
                assert_eq!(required_bytes, 3_000_000);
                assert_eq!(available_bytes, 2_000_000);
            }
            _ => panic!("expected QuotaExceeded error"),
        }
    }
}
