//! Jottacloud-powered providers. Telia Sky is the primary target; the
//! architecture keeps the provider a parameter so white-label services can be
//! added without touching the rest of Sky Core.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    TeliaSkyNo,
    TeliaCloudSe,
    Jottacloud,
    ElkjopNo,
    Tele2Se,
    Onlime,
    PhoneroSky,
}

impl Provider {
    pub const ALL: [Provider; 7] = [
        Provider::TeliaSkyNo,
        Provider::TeliaCloudSe,
        Provider::Jottacloud,
        Provider::ElkjopNo,
        Provider::Tele2Se,
        Provider::Onlime,
        Provider::PhoneroSky,
    ];

    /// Value of rclone's `config_traditional` white-label option
    /// (verified against rclone v1.75 `jottacloud` backend).
    pub fn rclone_whitelabel(self) -> &'static str {
        match self {
            Provider::TeliaSkyNo => "telia_no",
            Provider::TeliaCloudSe => "telia_se",
            Provider::Jottacloud => "jottacloud",
            Provider::ElkjopNo => "elkjop",
            Provider::Tele2Se => "tele2",
            Provider::Onlime => "onlime",
            Provider::PhoneroSky => "phonero",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Provider::TeliaSkyNo => "Telia Sky",
            Provider::TeliaCloudSe => "Telia Cloud",
            Provider::Jottacloud => "Jottacloud",
            Provider::ElkjopNo => "Elkjøp Cloud",
            Provider::Tele2Se => "Tele2 Cloud",
            Provider::Onlime => "Onlime",
            Provider::PhoneroSky => "Phonero Sky",
        }
    }

    /// Whether the provider is officially supported (tested) by Sky Linux.
    /// Others are exposed as "experimental".
    pub fn is_supported(self) -> bool {
        matches!(self, Provider::TeliaSkyNo)
    }

    /// Default rclone remote name for this provider.
    pub fn remote_name(self) -> String {
        format!("sky-{}", self.rclone_whitelabel().replace('_', "-"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_names_are_valid_rclone_names() {
        for p in Provider::ALL {
            let n = p.remote_name();
            assert!(
                n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{n}"
            );
        }
    }

    #[test]
    fn telia_sky_is_the_supported_default() {
        assert!(Provider::TeliaSkyNo.is_supported());
        assert_eq!(Provider::TeliaSkyNo.rclone_whitelabel(), "telia_no");
    }
}
