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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserKeySlot {
    #[serde(rename = "type")]
    pub slot_type: String,
    pub data: serde_json::Map<String, serde_json::Value>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightSection {
    pub colors: Vec<String>,
    pub config: LightConfig,
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

/// Embedded default profile (factory export).
pub fn default_profile_document() -> ProfileDocument {
    serde_json::from_str(include_str!("../resources/default_profile.json"))
        .expect("default_profile.json must match ProfileDocument schema")
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
