use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelGen {
    SandyBridge,  // Gen 6  – HD 2000/3000
    IvyBridge,    // Gen 7  – HD 2500/4000
    Haswell,      // Gen 7.5 – HD 4x00
    Broadwell,    // Gen 8  – HD 5500/6000
    Skylake,      // Gen 9  – HD 510/530
    KabyLake,     // Gen 9.5 – HD 620/630
    CoffeeLake,   // Gen 9.5 – UHD 630
    WhiskeyLake,  // Gen 9.5 – UHD 620 (8th gen U)
    CannonLake,   // Gen 10 – UHD 620 (10nm)
    IceLake,      // Gen 11 – Iris Plus G4/G7
    TigerLake,    // Gen 12 – Iris Xe / UHD 750
    AlderLake,    // Gen 12 – UHD 770
    Unknown,
}

impl IntelGen {
    pub fn from_device_id(did: u16) -> Self {
        match did {
            0x0102 | 0x0106 | 0x010A | 0x0112 | 0x0116 | 0x0122 | 0x0126 => Self::SandyBridge,
            0x0152 | 0x0156 | 0x015A | 0x0162 | 0x0166 | 0x016A         => Self::IvyBridge,
            0x0402 | 0x0406 | 0x040A | 0x040B | 0x040E | 0x041E | 0x0416 |
            0x0422 | 0x0426 | 0x042A | 0x042B | 0x042E | 0x043B | 0x0D22 |
            0x0D26 | 0x0D2A | 0x0D2B | 0x0D2E | 0x0D32 | 0x0D3A        => Self::Haswell,
            0x1602 | 0x1606 | 0x160A | 0x160B | 0x160D | 0x160E | 0x1612 |
            0x1616 | 0x161A | 0x161B | 0x161D | 0x161E | 0x1622 | 0x1626 |
            0x162A | 0x162B | 0x162D | 0x162E                           => Self::Broadwell,
            0x1902 | 0x1906 | 0x190A | 0x190B | 0x190E | 0x1912 | 0x1913 |
            0x1915 | 0x1916 | 0x191A | 0x191B | 0x191D | 0x191E | 0x1921 |
            0x1923 | 0x1926 | 0x1927 | 0x192A | 0x192B | 0x192D | 0x1932 |
            0x193A | 0x193B | 0x193D                                    => Self::Skylake,
            0x5902 | 0x5906 | 0x5908 | 0x590A | 0x590B | 0x590E | 0x5912 |
            0x5913 | 0x5915 | 0x5916 | 0x591A | 0x591B | 0x591C | 0x591D |
            0x591E | 0x5921 | 0x5923 | 0x5926 | 0x5927 | 0x593B        => Self::KabyLake,
            0x3E90 | 0x3E91 | 0x3E92 | 0x3E93 | 0x3E94 | 0x3E96 | 0x3E98 |
            0x3E99 | 0x3E9A | 0x3E9B | 0x3E9C | 0x3EA0 ..= 0x3EAF     => {
                // Disambiguate Coffee vs Whiskey Lake by sub-device heuristic
                if did >= 0x3EA0 { Self::WhiskeyLake } else { Self::CoffeeLake }
            }
            0x5A40 | 0x5A41 | 0x5A42 | 0x5A49 | 0x5A4A | 0x5A50 | 0x5A51 |
            0x5A59 | 0x5A5A | 0x5A5C                                    => Self::CannonLake,
            0x8A50 | 0x8A51 | 0x8A52 | 0x8A53 | 0x8A54 | 0x8A56 | 0x8A57 |
            0x8A58 | 0x8A59 | 0x8A5A | 0x8A5B | 0x8A5C | 0x8A5D | 0x8A70 |
            0x8A71                                                      => Self::IceLake,
            0x9A40 | 0x9A49 | 0x9A59 | 0x9A60 | 0x9A68 | 0x9A70 | 0x9A78 => Self::TigerLake,
            0x4600 | 0x4601 | 0x4602 | 0x4628 | 0x462A | 0x4636 | 0x4638 |
            0x463A | 0x4676 | 0x4678 | 0x467A | 0x46A0 | 0x46A1 | 0x46A2 |
            0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x46B0 | 0x46B1 | 0x46B2 |
            0x46B3 | 0x46C0 | 0x46C1 | 0x46C2 | 0x46C3 | 0x46D0 | 0x46D1 |
            0x46D2 | 0x4680 | 0x4682 | 0x4688 | 0x468A | 0x4690 | 0x4692 |
            0x4693 | 0x46A4 | 0x46A5                                    => Self::AlderLake,
            _ => Self::Unknown,
        }
    }

    pub fn mesa_driver(&self) -> &'static str {
        match self {
            Self::SandyBridge | Self::IvyBridge | Self::Haswell | Self::Broadwell => "crocus",
            _ => "iris",
        }
    }

    pub fn supports_vulkan(&self) -> bool {
        !matches!(self, Self::SandyBridge | Self::IvyBridge)
    }

    pub fn supports_vrr(&self) -> bool {
        matches!(self, Self::IceLake | Self::TigerLake | Self::AlderLake)
    }

    pub fn prefers_xe_driver(&self) -> bool {
        matches!(self, Self::TigerLake | Self::AlderLake)
    }

    pub fn marketing_name(&self) -> &'static str {
        match self {
            Self::SandyBridge => "Intel HD Graphics (Sandy Bridge, Gen 6)",
            Self::IvyBridge   => "Intel HD Graphics (Ivy Bridge, Gen 7)",
            Self::Haswell     => "Intel HD Graphics 4x00 (Haswell, Gen 7.5)",
            Self::Broadwell   => "Intel HD/Iris Graphics 5x00/6100 (Broadwell, Gen 8)",
            Self::Skylake     => "Intel HD Graphics 5xx/530 (Skylake, Gen 9)",
            Self::KabyLake    => "Intel HD Graphics 620/630 (Kaby Lake, Gen 9.5)",
            Self::CoffeeLake  => "Intel UHD Graphics 630 (Coffee Lake, Gen 9.5)",
            Self::WhiskeyLake => "Intel UHD Graphics 620 (Whiskey Lake, Gen 9.5)",
            Self::CannonLake  => "Intel UHD Graphics 620 (Cannon Lake, Gen 10)",
            Self::IceLake     => "Intel Iris Plus Graphics G4/G7 (Ice Lake, Gen 11)",
            Self::TigerLake   => "Intel Iris Xe / UHD 750 (Tiger Lake, Gen 12)",
            Self::AlderLake   => "Intel UHD Graphics 770 (Alder Lake, Gen 12)",
            Self::Unknown     => "Intel Graphics (unknown generation)",
        }
    }
}

pub fn log_intel_backend(did: u16) {
    let gen = IntelGen::from_device_id(did);
    info!(
        device_id  = format!("{did:#06x}"),
        marketing  = gen.marketing_name(),
        mesa       = gen.mesa_driver(),
        vulkan     = gen.supports_vulkan(),
        vrr        = gen.supports_vrr(),
        xe_driver  = gen.prefers_xe_driver(),
        "Intel GPU backend"
    );
}
