<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import type { DeviceStatus } from "$lib/types/profile";

  let { children }: { children: import("svelte").Snippet } = $props();

  let status = $state<DeviceStatus | null>(null);

  async function refreshStatus() {
    try {
      status = await invoke<DeviceStatus>("get_device_status");
    } catch {
      status = null;
    }
  }

  onMount(() => {
    refreshStatus();
    const id = setInterval(refreshStatus, 5000);
    return () => clearInterval(id);
  });

  const nav = [
    { href: "/lighting", label: "Lighting" },
    { href: "/performance", label: "Performance" },
    { href: "/keys", label: "Keys" },
    { href: "/profiles", label: "Profiles" },
  ];

  let path = $derived($page.url.pathname);
</script>

<div class="app">
  <aside class="sidebar">
    <div class="brand">CNTRL</div>
    <nav class="nav">
      {#each nav as item}
        <a href={item.href} class="nav-item" class:active={path === item.href} data-sveltekit-preload-data="tap"
          >{item.label}</a
        >
      {/each}
    </nav>
  </aside>

  <div class="main">
    <header class="statusbar">
      <span class="stat">
        <span class="dot" class:on={status?.connected} class:off={status && !status.connected}></span>
        {status?.connected ? "Keyboard" : "No device"} · {status?.mockMode ? "mock HID" : "linux HID"}
      </span>
      <span class="stat">Profile: {status?.profileName ?? "—"}</span>
      <span class="stat">FW: {status?.firmwareVersion ?? "—"}</span>
      <span class="stat">Slot: {(status?.activeSlot ?? 0) + 1}/4</span>
    </header>

    <main class="content">
      {@render children()}
    </main>
  </div>
</div>

<style>
  .app {
    display: flex;
    min-height: 100vh;
    background: var(--bg);
  }

  .sidebar {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    background: var(--surface);
    padding: 1rem 0;
  }

  .brand {
    font-weight: 600;
    letter-spacing: 0.2em;
    padding: 0 1rem 1.25rem;
    border-bottom: 1px solid var(--border);
    color: var(--accent);
    font-size: 14px;
  }

  .nav {
    display: flex;
    flex-direction: column;
    padding-top: 0.75rem;
  }

  .nav-item {
    display: block;
    padding: 0.55rem 1rem;
    color: var(--text-dim);
    border-left: 2px solid transparent;
    text-decoration: none;
  }

  .nav-item:hover {
    color: var(--text);
    background: rgba(0, 255, 136, 0.06);
    text-decoration: none;
  }

  .nav-item.active {
    color: var(--accent);
    border-left-color: var(--accent);
    background: rgba(0, 255, 136, 0.04);
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .statusbar {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem 1.5rem;
    padding: 0.5rem 1.25rem;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    color: var(--text-dim);
  }

  .stat {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }

  .dot {
    width: 6px;
    height: 6px;
    background: var(--border);
  }

  .dot.on {
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent);
  }

  .dot.off {
    background: #553333;
  }

  .content {
    flex: 1;
    padding: 1.25rem;
    overflow: auto;
  }
</style>
