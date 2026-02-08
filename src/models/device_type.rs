use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeviceType {
    HS100,
    HS103,
    HS105,
    HS110,
    HS200,
    HS300,
    HS300Child,
    KP115,
    KP125,
    KP200,
    KP200Child,
    KP303,
    KP303Child,
    KP400,
    KP400Child,
    KL420L5,
    KL430,
    EP40,
    EP40Child,
    Unknown,
}

/// Model prefix to DeviceType mapping, ordered by specificity (longest prefix first).
const MODEL_MAP: &[(&str, DeviceType)] = &[
    ("KL420L5", DeviceType::KL420L5),
    ("KL430", DeviceType::KL430),
    ("HS100", DeviceType::HS100),
    ("HS103", DeviceType::HS103),
    ("HS105", DeviceType::HS105),
    ("HS110", DeviceType::HS110),
    ("HS200", DeviceType::HS200),
    ("HS300", DeviceType::HS300),
    ("KP115", DeviceType::KP115),
    ("KP125", DeviceType::KP125),
    ("KP200", DeviceType::KP200),
    ("KP303", DeviceType::KP303),
    ("KP400", DeviceType::KP400),
    ("EP40", DeviceType::EP40),
];

impl DeviceType {
    pub fn from_model(model: &str) -> Self {
        for (prefix, device_type) in MODEL_MAP {
            if model.starts_with(prefix) {
                return *device_type;
            }
        }
        DeviceType::Unknown
    }

    pub fn child_type(&self) -> Self {
        match self {
            DeviceType::HS300 => DeviceType::HS300Child,
            DeviceType::KP200 => DeviceType::KP200Child,
            DeviceType::KP303 => DeviceType::KP303Child,
            DeviceType::KP400 => DeviceType::KP400Child,
            DeviceType::EP40 => DeviceType::EP40Child,
            _ => DeviceType::Unknown,
        }
    }

    pub fn has_children(&self) -> bool {
        matches!(
            self,
            DeviceType::HS300
                | DeviceType::KP200
                | DeviceType::KP303
                | DeviceType::KP400
                | DeviceType::EP40
        )
    }

    pub fn has_emeter(&self) -> bool {
        matches!(
            self,
            DeviceType::HS110 | DeviceType::KP115 | DeviceType::KP125 | DeviceType::HS300Child
        )
    }

    pub fn is_light(&self) -> bool {
        matches!(self, DeviceType::KL420L5 | DeviceType::KL430)
    }

    pub fn is_child(&self) -> bool {
        matches!(
            self,
            DeviceType::HS300Child
                | DeviceType::KP200Child
                | DeviceType::KP303Child
                | DeviceType::KP400Child
                | DeviceType::EP40Child
        )
    }

    pub fn category(&self) -> &'static str {
        if self.is_light() {
            "light"
        } else if matches!(self, DeviceType::HS200) {
            "switch"
        } else {
            "plug"
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            DeviceType::HS100 => "HS100",
            DeviceType::HS103 => "HS103",
            DeviceType::HS105 => "HS105",
            DeviceType::HS110 => "HS110",
            DeviceType::HS200 => "HS200",
            DeviceType::HS300 => "HS300",
            DeviceType::HS300Child => "HS300 Outlet",
            DeviceType::KP115 => "KP115",
            DeviceType::KP125 => "KP125",
            DeviceType::KP200 => "KP200",
            DeviceType::KP200Child => "KP200 Outlet",
            DeviceType::KP303 => "KP303",
            DeviceType::KP303Child => "KP303 Outlet",
            DeviceType::KP400 => "KP400",
            DeviceType::KP400Child => "KP400 Outlet",
            DeviceType::KL420L5 => "KL420L5",
            DeviceType::KL430 => "KL430",
            DeviceType::EP40 => "EP40",
            DeviceType::EP40Child => "EP40 Outlet",
            DeviceType::Unknown => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        assert_eq!(DeviceType::from_model("HS100(US)"), DeviceType::HS100);
        assert_eq!(DeviceType::from_model("KP115(US)"), DeviceType::KP115);
        assert_eq!(DeviceType::from_model("KL430(US)"), DeviceType::KL430);
        assert_eq!(DeviceType::from_model("HS300(US)"), DeviceType::HS300);
        assert_eq!(DeviceType::from_model("UNKNOWN_MODEL"), DeviceType::Unknown);
    }

    #[test]
    fn test_has_children() {
        assert!(DeviceType::HS300.has_children());
        assert!(DeviceType::KP303.has_children());
        assert!(!DeviceType::HS100.has_children());
        assert!(!DeviceType::KL430.has_children());
    }

    #[test]
    fn test_has_emeter() {
        assert!(DeviceType::HS110.has_emeter());
        assert!(DeviceType::KP115.has_emeter());
        assert!(DeviceType::HS300Child.has_emeter());
        assert!(!DeviceType::HS100.has_emeter());
    }

    #[test]
    fn test_is_light() {
        assert!(DeviceType::KL430.is_light());
        assert!(DeviceType::KL420L5.is_light());
        assert!(!DeviceType::HS100.is_light());
    }

    #[test]
    fn test_child_type() {
        assert_eq!(DeviceType::HS300.child_type(), DeviceType::HS300Child);
        assert_eq!(DeviceType::KP303.child_type(), DeviceType::KP303Child);
    }
}
