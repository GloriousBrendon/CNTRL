pub const EFFECT_ID_OFF: i32 = 255;
pub const MODE_CUSTOM: i32 = -1;
pub const MODE_STATIC: i32 = -2;

#[derive(Clone, Copy)]
pub struct PresetMode {
    pub slot: u8,
    pub label: &'static str,
    pub effect_id: u8,
}

pub const PRESET_MODES: [PresetMode; 22] = [
    PresetMode { slot: 1, label: "Tidal rhythm", effect_id: 6 },
    PresetMode { slot: 2, label: "Surging light tide", effect_id: 7 },
    PresetMode { slot: 3, label: "Colorful fountain", effect_id: 8 },
    PresetMode { slot: 4, label: "Galaxy roaming", effect_id: 9 },
    PresetMode { slot: 5, label: "Light pulsation", effect_id: 4 },
    PresetMode { slot: 6, label: "Aurora mapping", effect_id: 1 },
    PresetMode { slot: 7, label: "Flower dance", effect_id: 5 },
    PresetMode { slot: 8, label: "Serene Realm", effect_id: 3 },
    PresetMode { slot: 9, label: "Shadow steps", effect_id: 2 },
    PresetMode { slot: 10, label: "Surge rhythm", effect_id: 11 },
    PresetMode { slot: 11, label: "Ocean", effect_id: 12 },
    PresetMode { slot: 12, label: "Light spiral", effect_id: 10 },
    PresetMode { slot: 13, label: "Flow dance", effect_id: 18 },
    PresetMode { slot: 14, label: "Raining", effect_id: 19 },
    PresetMode { slot: 15, label: "Starlight", effect_id: 20 },
    PresetMode { slot: 16, label: "Fireworks", effect_id: 21 },
    PresetMode { slot: 17, label: "Ripple flow", effect_id: 22 },
    PresetMode { slot: 18, label: "Light ripple", effect_id: 14 },
    PresetMode { slot: 19, label: "Ripple dance", effect_id: 13 },
    PresetMode { slot: 20, label: "Instant flash", effect_id: 15 },
    PresetMode { slot: 21, label: "Matrix shadows", effect_id: 16 },
    PresetMode { slot: 22, label: "Melody piano", effect_id: 17 },
];

pub fn is_preset_effect_id(effect_id: i32) -> bool {
    PRESET_MODES
        .iter()
        .any(|m| i32::from(m.effect_id) == effect_id)
}
