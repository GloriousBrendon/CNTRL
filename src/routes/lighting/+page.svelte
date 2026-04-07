<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { KeyboardProfile, LightPresetEntry } from "$lib/types/profile";

  let profile = $state<KeyboardProfile | null>(null);
  let err = $state<string | null>(null);

  let hex = $state("#00ff88");
  let brightness = $state(100);
  let speed = $state(0);
  let effect = $state(3);

  const effectLabels: Record<number, string> = {
    0: "Custom",
    3: "Static",
    4: "Breath",
    6: "Wave",
    7: "Ripple",
    8: "Rainbow wave",
    9: "Spectrum",
    14: "Rotate",
    15: "Flow",
    16: "Flame",
    17: "Circle",
    20: "Aurora",
    21: "Music",
    22: "Reactive",
    255: "Off",
  };

  function labelForPreset(p: LightPresetEntry): string {
    return effectLabels[p.effect] ?? `Effect ${p.effect}`;
  }

  async function load() {
    try {
      err = null;
      profile = await invoke<KeyboardProfile>("get_profile");
      const c = profile!.data.light.config;
      hex = c.hexColor;
      brightness = c.brightness;
      speed = c.speed;
      effect = c.effect;
    } catch (e) {
      err = String(e);
    }
  }

  async function pushLighting() {
    try {
      err = null;
      profile = await invoke<KeyboardProfile>("set_lighting", {
        hexColor: hex,
        effect: Number(effect),
        brightness: Number(brightness),
        speed: Number(speed),
      });
    } catch (e) {
      err = String(e);
    }
  }

  async function applyPreset(p: LightPresetEntry) {
    hex = p.hexColor;
    effect = p.effect;
    brightness = p.brightness;
    speed = p.speed;
    await pushLighting();
    try {
      profile = await invoke<KeyboardProfile>("set_selected_light_effect", {
        effectId: p.effect === 3 ? "still" : `fx-${p.effect}`,
        displayName: labelForPreset(p),
      });
    } catch {
      /* optional */
    }
  }

  onMount(load);
</script>

<div class="page">
  <h1>Lighting</h1>
  {#if err}
    <p class="err">{err}</p>
  {/if}

  {#if profile}
    <div class="grid">
      <section class="panel">
        <h2>Solid & global</h2>
        <div class="row controls">
          <div>
            <span class="label">Color</span>
            <div class="row">
              <input type="color" bind:value={hex} onchange={pushLighting} class="color-input" />
              <input
                type="text"
                class="hex-field"
                bind:value={hex}
                maxlength="7"
                onchange={pushLighting}
              />
            </div>
          </div>
          <div class="grow">
            <span class="label">Brightness ({brightness})</span>
            <input
              type="range"
              min="0"
              max="100"
              bind:value={brightness}
              oninput={pushLighting}
              class="slider"
            />
          </div>
          <div class="grow">
            <span class="label">Speed ({speed})</span>
            <input type="range" min="0" max="5" bind:value={speed} oninput={pushLighting} class="slider" />
          </div>
        </div>

        <div class="row" style="margin-top: 1rem">
          <div>
            <span class="label">Effect ID</span>
            <input
              type="number"
              class="num"
              bind:value={effect}
              onchange={pushLighting}
              min="0"
              max="255"
            />
          </div>
          <p class="hint">
            Static = 3. Presets below set effect, color, brightness, and speed together.
          </p>
        </div>
      </section>

      <section class="panel">
        <h2>Presets (from profile)</h2>
        <div class="preset-grid">
          {#each profile.data.lightValueStore.light as p}
            <button type="button" class="preset" onclick={() => applyPreset(p)}>
              <span class="swatch" style="background-color: {p.hexColor}"></span>
              <span class="preset-label">{labelForPreset(p)}</span>
              <span class="preset-meta">#{p.effect} · b{p.brightness} · s{p.speed}</span>
            </button>
          {/each}
        </div>
      </section>

      <section class="panel full">
        <h2>Selected effect</h2>
        <code class="mono"
          >{profile.data.selectedLightEffect.join(" · ") || "—"}</code
        >
      </section>
    </div>
  {:else}
    <p class="dim">Loading profile…</p>
  {/if}
</div>

<style>
  .page {
    max-width: 960px;
  }

  .grid {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .panel.full {
    width: 100%;
  }

  h1 {
    font-size: 1.1rem;
    text-transform: uppercase;
    letter-spacing: 0.18em;
    color: var(--text-dim);
    margin-bottom: 1rem;
  }

  h2 {
    font-size: 0.85rem;
    margin-bottom: 0.75rem;
    color: var(--text);
  }

  .controls {
    align-items: flex-end;
  }

  .grow {
    flex: 1;
    min-width: 160px;
  }

  .color-input {
    width: 48px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--border);
    background: var(--bg);
    cursor: pointer;
  }

  .hex-field {
    width: 88px;
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
  }

  .slider {
    width: 100%;
    accent-color: var(--accent);
  }

  .num {
    width: 72px;
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
  }

  .hint {
    margin: 0;
    font-size: 11px;
    color: var(--text-dim);
    max-width: 28rem;
  }

  .preset-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 0.5rem;
  }

  .preset {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
    padding: 0.5rem 0.65rem;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    text-align: left;
  }

  .preset:hover {
    border-color: var(--accent);
  }

  .swatch {
    width: 100%;
    height: 10px;
    border: 1px solid var(--border);
  }

  .preset-label {
    font-size: 12px;
  }

  .preset-meta {
    font-size: 10px;
    color: var(--text-dim);
  }

  .mono {
    font-size: 12px;
    color: var(--accent);
  }

  .err {
    color: #ff6b6b;
    margin: 0 0 1rem;
  }

  .dim {
    color: var(--text-dim);
  }
</style>
