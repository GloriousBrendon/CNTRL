/** Subset of Ace68-II profile JSON used by the UI. */
export interface LightConfig {
  hexColor: string;
  effect: number;
  brightness: number;
  speed: number;
}

export interface LightPresetEntry {
  hexColor: string;
  speed: number;
  brightness: number;
  direct: number;
  customColorDisabled: number;
  effect: number;
}

export interface ProfileData {
  name: string;
  light: {
    colors: string[];
    config: LightConfig;
  };
  lightValueStore: {
    light: LightPresetEntry[];
  };
  selectedLightEffect: string[];
  extra: {
    fw_ver: string;
  };
}

export interface KeyboardProfile {
  data: ProfileData;
}

export interface DeviceStatus {
  connected: boolean;
  profileName: string;
  firmwareVersion: string;
  activeSlot: number;
  mockMode: boolean;
}
