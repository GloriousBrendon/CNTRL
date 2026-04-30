#!/usr/bin/env python3
"""Compare effect 6 template from JSON vs pcap. Check if zone continuation exists."""
import json, subprocess

with open('src-tauri/resources/effects_light_v2.json') as f:
    data = json.load(f)

# Effect 6 from JSON
eff6_hex = data['templates']['6']
eff6 = bytes.fromhex(eff6_hex)
print("=== EFFECT 6 FROM effects_light_v2.json ===")
for i in range(0, len(eff6), 16):
    chunk = eff6[i:i+16]
    print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

print(f"\nTemplate length: {len(eff6)} bytes, {len(eff6_hex)} hex chars")

# From pcap: effect 6 profile (frame 485)
pcap_eff6 = bytes.fromhex("55f200cd38d802003c6c6967687440763e3e10000006ff00000064"
    "11ff00000064"
    "10ff00000064"
    "0fff00000064"
    "0dff00000064"
    "0eff00000064"
    "16ff00000064"
    "15")
# Wait, that's not right. Let me use the exact hex from the pcap.

print("\n=== PCAP EFFECT 6 PROFILE (frame 485) ===")
# Rebuild from hex dump:
pcap_hex = "55f200cd38d802003c6c696768744076323e1000000" + "6ff000000641" + "1ff000000641" + "0ff000000640" + "fff000000640" + "dff000000640" + "eff000000641" + "6ff0000006415"
    
# Actually let me just extract it properly
result = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields',
     '-e', 'frame.number',
     '-e', 'usb.capdata',
     '-e', 'usbhid.data',
     '-Y', 'frame.number == 485 || frame.number == 489 || frame.number == 493 || frame.number == 497'],
    capture_output=True, text=True, cwd='.')

for line in result.stdout.strip().split('\n'):
    parts = line.split('\t')
    frame = parts[0]
    data = (parts[1] or parts[2]).replace(':', '').lower()
    raw = bytes.fromhex(data)
    print(f"\nFrame {frame} ({len(raw)} bytes):")
    for i in range(0, len(raw), 16):
        chunk = raw[i:i+16]
        print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# Compare byte by byte
print("\n=== BYTE-BY-BYTE COMPARISON: JSON template vs pcap frame 485 ===")
# Get the pcap data
for line in result.stdout.strip().split('\n'):
    parts = line.split('\t')
    if parts[0].strip() == '485':
        pcap_data = (parts[1] or parts[2]).replace(':', '').lower()
        pcap_raw = bytes.fromhex(pcap_data)
        break

diffs = []
for i in range(64):
    if eff6[i] != pcap_raw[i]:
        diffs.append(i)
        
if not diffs:
    print("IDENTICAL! Effect 6 template matches pcap profile packet exactly.")
else:
    print(f"Differences at bytes: {diffs}")
    for i in diffs:
        print(f"  byte[{i}]: json={eff6[i]:02x}  pcap={pcap_raw[i]:02x}")

# Now the key question: the zone packet (frame 489) uses byte[5]=0x0d.
# Does the code need a DIFFERENT zone template for effects?
# Let me check: is the zone data for each effect DIFFERENT?
print("\n=== DO DIFFERENT EFFECTS HAVE DIFFERENT ZONE PACKETS? ===")
result2 = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields', '-e', 'frame.number', '-e', 'usb.capdata', '-e', 'usbhid.data',
     '-Y', 'usb.data_len > 0'],
    capture_output=True, text=True, cwd='.')

# Collect all 55f2 packets with b5=0x0d (effect zones)
zones = []
for line in result2.stdout.strip().split('\n'):
    parts = line.split('\t')
    if len(parts) < 3: continue
    data = (parts[1] or parts[2] or '').replace(':', '').lower()
    if not data or len(data) < 12: continue
    if data[:2] == '55' and data[2:4] == 'f2':
        raw = bytes.fromhex(data)
        if raw[5] == 0x0d:
            zones.append((int(parts[0]), data))

print(f"\nFound {len(zones)} effect zone packets")
# Show bytes 8-63 of each to see if they differ
unique_zones = set()
for frame, hx in zones:
    raw = bytes.fromhex(hx) 
    # bytes 8+ is the zone data
    zone_data = hx[16:]  # skip first 8 bytes
    unique_zones.add(zone_data)
    print(f"  frame {frame}: zone_data_start = {zone_data[:30]}...")

print(f"\nUnique zone data payloads: {len(unique_zones)}")
print("(If all 22 are unique, zone data is effect-specific and we need a template for each)")
print("(If they're all the same, we can use one shared zone template)")
