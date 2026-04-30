#!/usr/bin/env python3
"""Extract the 22 zone continuation packets from pcap and format for effects_light_v2.json."""
import subprocess, json

result = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields', '-e', 'frame.number', '-e', 'usb.capdata', '-e', 'usbhid.data',
     '-Y', 'usb.data_len > 0'],
    capture_output=True, text=True, cwd='.')

# Collect ALL 0x55 sends
sends = []
for line in result.stdout.strip().split('\n'):
    parts = line.split('\t')
    if len(parts) < 3: continue
    data = (parts[1] or parts[2] or '').replace(':', '').lower()
    if data and data[:2] == '55':
        sends.append((int(parts[0]), data))

# Find each effect's pipeline: PROFILE -> ZONE -> APPLY_A -> APPLY_B
# Profile packets: 55 f2 with b5=0xd8 
# Zone packets: 55 f2 with b5=0x0d
# Apply_A: 55 06 with b4=0x38 b5=0x00
# Apply_B: 55 06 with b4=0x38 b5=0x08

effect_zones = {}  # effect_id -> zone hex
effect_apply_a = {}  # effect_id -> apply_a hex
effect_apply_b = {}  # effect_id -> apply_b hex

i = 0
while i < len(sends):
    frame, hx = sends[i]
    raw = bytes.fromhex(hx)
    
    # Is this a profile packet? (55 f2, b5=d8)
    if raw[1] == 0xf2 and len(raw) >= 22 and raw[5] == 0xd8:
        effect_id = raw[21]
        # Next 55f2 should be zone (b5=0x0d)
        for j in range(i+1, min(i+8, len(sends))):
            _, z_hx = sends[j]
            z_raw = bytes.fromhex(z_hx)
            if z_raw[1] == 0xf2 and z_raw[5] == 0x0d:
                effect_zones[effect_id] = z_hx
                break
        # Find apply_a and apply_b
        for j in range(i+1, min(i+12, len(sends))):
            _, a_hx = sends[j]
            a_raw = bytes.fromhex(a_hx)
            if a_raw[1] == 0x06 and a_raw[5] == 0x00 and effect_id not in effect_apply_a:
                effect_apply_a[effect_id] = a_hx
            elif a_raw[1] == 0x06 and a_raw[5] == 0x08 and effect_id not in effect_apply_b:
                effect_apply_b[effect_id] = a_hx
    i += 1

print(f"Found zone packets for {len(effect_zones)} effects")
print(f"Found apply_a for {len(effect_apply_a)} effects")  
print(f"Found apply_b for {len(effect_apply_b)} effects")

# Check: what bytes differ between apply_a packets?
print("\n=== APPLY_A DIFFERENCES BETWEEN EFFECTS ===")
for eid in sorted(effect_apply_a.keys()):
    raw = bytes.fromhex(effect_apply_a[eid])
    print(f"  effect {eid:2d}: b9={raw[9]:02x} b16={raw[16]:02x} b22-24={raw[22]:02x}{raw[23]:02x}{raw[24]:02x}")

print("\n=== APPLY_B DIFFERENCES BETWEEN EFFECTS ===")
for eid in sorted(effect_apply_b.keys()):
    raw = bytes.fromhex(effect_apply_b[eid])
    print(f"  effect {eid:2d}: b8={raw[8]:02x} b9={raw[9]:02x} b14-16={raw[14]:02x}{raw[15]:02x}{raw[16]:02x}")

# Output zone templates for inclusion in effects_light_v2.json
print("\n=== ZONE TEMPLATES FOR effects_light_v2.json ===")
zones_json = {}
for eid in sorted(effect_zones.keys()):
    zones_json[str(eid)] = effect_zones[eid]
    
print(json.dumps({"zones": zones_json}, indent=2))
