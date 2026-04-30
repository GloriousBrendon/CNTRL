//! Ace68-II keyboard profile JSON (KeyboardProfile v3).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDocument {
    pub data_scope: String,
    #[serde(rename = "type")]
    pub keyboard_type: String,
    pub version: i64,
    pub vendor_id: i64,
    pub product_id: i64,
    pub product_name: String,
    pub identity: ProductIdentity,
    pub exported_at: i64,
    pub data: ProfileData,
    pub macros: Vec<MacroEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductIdentity {
    pub vendor_id: i64,
    pub product_id: i64,
    pub product_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileData {
    pub user_keys: Vec<UserKeySlot>,
    pub advanced_keys: AdvancedKeys,
    pub performance: Performance,
    pub light_value_store: LightValueStore,
    pub light: LightSection,
    pub trigger_travel: TriggerTravel,
    pub custom_param: CustomParam,
    /// e.g. `["still", "Static"]`
    pub selected_light_effect: Vec<String>,
    pub name: String,
    pub extra: ProfileExtra,
}

/// One FN / layer slice of the keymap (128 logical slots; Ace68 uses 68 physical keys).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserKeyLayerData {
    #[serde(default)]
    pub keys: Vec<KeyDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserKeySlot {
    #[serde(rename = "type")]
    pub slot_type: String,
    #[serde(default)]
    pub data: UserKeyLayerData,
}

/// Single key assignment (HID-oriented), matching MCHOSE web exports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDef {
    #[serde(rename = "type")]
    pub key_type: i32,
    pub code1: i32,
    pub code2: i32,
    pub code: i32,
    pub name: String,
    pub index: i32,
    pub layer: i32,
}

impl KeyDef {
    pub fn empty(index: i32, layer: i32) -> Self {
        KeyDef {
            key_type: 0,
            code1: 0,
            code2: 0,
            code: 0,
            name: String::new(),
            index,
            layer,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedKeys {
    pub dks: Vec<DksEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DksAction {
    #[serde(rename = "type")]
    pub action_type: i32,
    pub code1: i32,
    pub code2: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DksPointActions {
    pub action0: i32,
    pub action1: i32,
    pub action2: i32,
    pub action3: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DksEntry {
    pub point: [i32; 4],
    pub action0: DksAction,
    pub action1: DksAction,
    pub action2: DksAction,
    pub action3: DksAction,
    pub point0: DksPointActions,
    pub point1: DksPointActions,
    pub point2: DksPointActions,
    pub point3: DksPointActions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performance {
    #[serde(rename = "macMode")]
    pub mac_mode: i32,
    #[serde(rename = "reporteRate")]
    pub reporte_rate: i32,
    #[serde(rename = "tickRate")]
    pub tick_rate: i32,
    #[serde(rename = "deadZone")]
    pub dead_zone: i32,
    #[serde(rename = "lockWin")]
    pub lock_win: i32,
    #[serde(rename = "lockAltTap")]
    pub lock_alt_tap: i32,
    #[serde(rename = "lockAltF4")]
    pub lock_alt_f4: i32,
    #[serde(rename = "tachyonMode")]
    pub tachyon_mode: i32,
    #[serde(rename = "bottomrapidtriggerMode")]
    pub bottomrapidtrigger_mode: i32,
    #[serde(rename = "travelCheck")]
    pub travel_check: i32,
    #[serde(rename = "debugMode")]
    pub debug_mode: i32,
    #[serde(rename = "debounceLevel")]
    pub debounce_level: i32,
    pub rtsmart: i32,
    #[serde(rename = "rf_battery")]
    pub rf_battery: i32,
    #[serde(rename = "isCharging")]
    pub is_charging: i32,
    #[serde(rename = "roller_type")]
    pub roller_type: i32,
    #[serde(rename = "work_mode")]
    pub work_mode: i32,
    #[serde(rename = "sleepTime")]
    pub sleep_time: i32,
    #[serde(rename = "sleepMode")]
    pub sleep_mode: i32,
    #[serde(rename = "rf_bt_ch")]
    pub rf_bt_ch: i32,
    #[serde(rename = "report_rate_2_4G")]
    pub report_rate_2_4g: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightValueStore {
    pub light: Vec<LightPresetEntry>,
    pub side_light: Vec<serde_json::Value>,
    pub side_light2: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightPresetEntry {
    #[serde(rename = "hexColor")]
    pub hex_color: String,
    pub speed: i32,
    pub brightness: i32,
    pub direct: i32,
    #[serde(rename = "customColorDisabled")]
    pub custom_color_disabled: i32,
    pub effect: i32,
}

/// Per-frame key colors for custom GIF / timeline lighting (IndexedDB shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifKeyColor {
    pub code: i32,
    pub select_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimationFrame {
    pub duration: u32,
    pub data: Vec<GifKeyColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightSection {
    pub colors: Vec<String>,
    pub config: LightConfig,
    /// Optional custom animation; omitted in factory JSON until set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gif_frames: Option<Vec<AnimationFrame>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationRgb {
    pub r: i32,
    pub g: i32,
    pub b: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightConfig {
    #[serde(rename = "type")]
    pub config_type: i32,
    pub effect: i32,
    pub brightness: i32,
    pub speed: i32,
    pub direct: i32,
    #[serde(rename = "customColorDisabled")]
    pub custom_color_disabled: i32,
    #[serde(rename = "colorIndex")]
    pub color_index: i32,
    #[serde(rename = "hexColor")]
    pub hex_color: String,
    #[serde(rename = "side_effect")]
    pub side_effect: i32,
    #[serde(rename = "side_brightness")]
    pub side_brightness: i32,
    #[serde(rename = "side_speed")]
    pub side_speed: i32,
    #[serde(rename = "sideCustomColorDisabled")]
    pub side_custom_color_disabled: i32,
    #[serde(rename = "side_hexColor")]
    pub side_hex_color: String,
    #[serde(rename = "calibrationRgb")]
    pub calibration_rgb: CalibrationRgb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerTravel {
    pub travel_keys: Vec<TravelKey>,
    pub status_keys: Vec<StatusKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelKey {
    #[serde(rename = "switchType")]
    pub switch_type: i32,
    #[serde(rename = "keyMode")]
    pub key_mode: i32,
    pub isolated: i32,
    pub priority: i32,
    #[serde(rename = "key_max_length")]
    pub key_max_length: i32,
    #[serde(rename = "keyActuation")]
    pub key_actuation: i32,
    #[serde(rename = "pressPrecision")]
    pub press_precision: i32,
    #[serde(rename = "releasePrecision")]
    pub release_precision: i32,
    #[serde(rename = "rtPress")]
    pub rt_press: i32,
    #[serde(rename = "rtRelease")]
    pub rt_release: i32,
    #[serde(rename = "pressDeadzone")]
    pub press_deadzone: i32,
    #[serde(rename = "releaseDeadzone")]
    pub release_deadzone: i32,
    #[serde(rename = "deadzoneStatus")]
    pub deadzone_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusKey {
    pub isolated: i32,
    #[serde(rename = "deadzoneStatus")]
    pub deadzone_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomParam {
    pub rt_press_precision_mode: i32,
    pub rt_release_precision_mode: i32,
    pub rt_smart_cache_value: i32,
    pub cb_key_index_list: Vec<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExtra {
    pub fw_ver: String,
    pub rffw_ver: String,
    pub identity: ProductIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroEntry {
    pub macro_index: i32,
    pub macro_actions: Vec<serde_json::Value>,
    pub name: String,
    #[serde(rename = "type")]
    pub macro_type: i32,
    pub default_delay: i32,
    pub enable_default_delay: bool,
}

pub const KEYMAP_LAYERS: usize = 16;
pub const KEYMAP_SLOTS: usize = 128;

/// Embedded default profile (factory export).
pub fn default_profile_document() -> ProfileDocument {
    let mut p: ProfileDocument = serde_json::from_str(include_str!("../../resources/default_profile.json"))
        .expect("default_profile.json must match ProfileDocument schema");
    normalize(&mut p);
    p
}

/// Pad `userKeys` to 16 layers and 128 keys per layer so remaps and HID packing are stable.
pub fn normalize(doc: &mut ProfileDocument) {
    while doc.data.user_keys.len() < KEYMAP_LAYERS {
        doc.data.user_keys.push(UserKeySlot {
            slot_type: "diff-keys".to_string(),
            data: UserKeyLayerData::default(),
        });
    }
    for (layer, slot) in doc.data.user_keys.iter_mut().enumerate() {
        while slot.data.keys.len() < KEYMAP_SLOTS {
            let i = slot.data.keys.len() as i32;
            slot.data.keys.push(KeyDef::empty(i, layer as i32));
        }
    }
    while doc.data.light.colors.len() < KEYMAP_SLOTS {
        doc.data.light.colors.push("#000000".to_string());
    }
    seed_ace68_layer0_if_empty(doc);
    mirror_layer0_to_empty_upper_layers(doc);
}

/// When layer 0 is still factory-empty (all codes 0), apply Ace68 code→slot mapping from MCHOSE exports
/// so per-key lighting and the UI grid resolve HID codes to `light.colors[index]`.
fn seed_ace68_layer0_if_empty(doc: &mut ProfileDocument) {
    if doc.data.user_keys.is_empty() {
        return;
    }
    let layer0 = &doc.data.user_keys[0].data.keys;
    if layer0.iter().any(|k| k.code != 0) {
        return;
    }
    let raw = include_str!("../../resources/ace68_layer0_code_index.json");
    let pairs: Vec<[i32; 2]> = serde_json::from_str(raw).expect("ace68_layer0_code_index.json");
    let layer0 = &mut doc.data.user_keys[0].data.keys;
    for pair in pairs {
        let code = pair[0];
        let idx = pair[1] as usize;
        if let Some(k) = layer0.get_mut(idx) {
            apply_hid_code(k, code as u16);
            k.key_type = 16;
            k.layer = 0;
            k.index = idx as i32;
        }
    }
}

/// Until the user customizes upper FN layers, mirror layer 0 so all 16 layers are addressable.
fn mirror_layer0_to_empty_upper_layers(doc: &mut ProfileDocument) {
    if doc.data.user_keys.len() < 2 {
        return;
    }
    let l0 = doc.data.user_keys[0].data.keys.clone();
    if l0.iter().all(|k| k.code == 0) {
        return;
    }
    for l in 1..KEYMAP_LAYERS {
        let empty = doc.data.user_keys[l]
            .data
            .keys
            .iter()
            .all(|k| k.code == 0);
        if !empty {
            continue;
        }
        doc.data.user_keys[l].data.keys = l0
            .iter()
            .map(|k| {
                let mut x = k.clone();
                x.layer = l as i32;
                x
            })
            .collect();
    }
}

/// Map HID usage `code` → linear index in `light.colors` using layer 0 assignments.
pub fn code_to_color_index(doc: &ProfileDocument, code: u8) -> Option<usize> {
    let layer0 = doc.data.user_keys.first()?;
    for k in &layer0.data.keys {
        if k.code == code as i32 {
            return Some(k.index as usize);
        }
    }
    None
}

/// Apply `code1` / `code2` heuristics used in exports for standard keys.
pub fn apply_hid_code(key: &mut KeyDef, code: u16) {
    let c = i32::from(code);
    key.code = c;
    if code <= 0xff {
        key.code1 = 0;
        key.code2 = c;
    } else {
        key.code1 = (c >> 8) & 0xff;
        key.code2 = c & 0xff;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_roundtrip() {
        let p = default_profile_document();
        let s = serde_json::to_string_pretty(&p).unwrap();
        let p2: ProfileDocument = serde_json::from_str(&s).unwrap();
        assert_eq!(p.data.light.config.hex_color, p2.data.light.config.hex_color);
        assert_eq!(p.macros.len(), 16);
    }
}
