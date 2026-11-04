//! One lifetime conversion for forms, prepared credentials and desired state.
use hellas_auth::authority::UnixMillis;
use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Period {
    Day,
    #[default]
    Week,
    Month,
    Year,
    Unlimited,
}
impl Period {
    pub const MAX_MS: u64 = 100 * 365 * 86_400_000;
    pub const fn millis(self) -> u64 {
        let days = match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
            Self::Unlimited => 100 * 365,
        };
        days * 86_400_000
    }
    pub fn expires(self, now: UnixMillis) -> anyhow::Result<UnixMillis> {
        Ok(UnixMillis(now.0.checked_add(self.millis()).ok_or_else(
            || anyhow::anyhow!("expiry exceeds clock range"),
        )?))
    }
    pub fn expires_within(
        self,
        now: UnixMillis,
        ceiling: UnixMillis,
    ) -> anyhow::Result<UnixMillis> {
        anyhow::ensure!(ceiling > now, "the authorizing grant has expired");
        Ok(self.expires(now)?.min(ceiling))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_fixed_periods_share_checked_conversion_and_self_issuance_ceiling() {
        assert_eq!(Period::default().millis(), 7 * 86_400_000);
        assert_eq!(Period::Unlimited.millis(), Period::MAX_MS);
        assert!(Period::Day.expires(UnixMillis(u64::MAX)).is_err());
        assert_eq!(
            Period::Unlimited
                .expires_within(UnixMillis(10), UnixMillis(20))
                .unwrap(),
            UnixMillis(20)
        );
        assert!(
            Period::Week
                .expires_within(UnixMillis(20), UnixMillis(20))
                .is_err()
        );
    }
}
