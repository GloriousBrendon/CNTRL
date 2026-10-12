#!/usr/bin/env python3
"""Extract full packets for effect 6 (first one in pcap) to compare with code."""
import subprocess, sys

result = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields',
     '-e', 'frame.number',
     '-e', 'usb.capdata',
     '-e', 'usbhid.data',
     '-Y', 'usb.data_len > 0'],
    capture_output=True, text=True, cwd='.')

lines = result.stdout.strip().split('\n')
sends = []
for line in lines:
    parts = line.split('\t')
    if len(parts) < 3:
        continue
    frame = int(parts[0])
    data = parts[1] or parts[2]
    if not data:
        continue
    clean = data.replace(':', '').replace(' ', '').lower()
    if clean[:2] == '55':
        sends.append((frame, clean))

# The first effect sequence starts at frame 485 (effect 6)
# Let's find the full pipeline for effect 6: frames 469-497
# Pipeline: GET_PERF -> PROFILE -> ZONE -> APPLY_A -> APPLY_B
# From the output: frames 469,473 (GET_PERF), 485 (PROFILE), 489 (ZONE), 493 (APPLY_A), 497 (APPLY_B)

print("=== FULL PACKETS FOR EFFECT 6 (first effect in pcap) ===\n")
for frame, hx in sends:
    if 460 <= frame <= 500:
        raw = bytes.fromhex(hx)
        print(f"frame {frame}: ({len(raw)} bytes)")
        # Print in groups of 16
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            hexpart = ' '.join(f'{b:02x}' for b in chunk)
            print(f"  [{i:3d}] {hexpart}")
        print()

# Also show what the code would generate
print("=== ZONE PACKET COMPARISON ===")
print("\nCode's ZONE_SINGLE (byte[5]=0x07):")
zone_single = bytes.fromhex("55f2003d380703001064000000ff1064ff0000ff1064110000ff1064100000ff10640f0000ff10640e0000ff1064160000ff1064150000ff1064140000ff1064")
for i in range(0, len(zone_single), 16):
    chunk = zone_single[i:i+16]
    print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

print("\nPcap's EFFECT ZONE (frame 489, byte[5]=0x0d):")
for frame, hx in sends:
    if frame == 489:
        raw = bytes.fromhex(hx)
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# Show the APPLY packets comparison
print("\n=== APPLY_A (effect) vs Code's build_apply_a ===")
print("Code builds apply_a from aa05 performance reply:")
print("  a[0]=0x55, a[1]=0x06, then stamps rgb at [22-24], recompute checksum")
print("\nPcap's APPLY_A for effect 6 (frame 493):")
for frame, hx in sends:
    if frame == 493:
        raw = bytes.fromhex(hx)
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

print("\nPcap's APPLY_B for effect 6 (frame 497):")
for frame, hx in sends:
    if frame == 497:
        raw = bytes.fromhex(hx)
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# Also check: what does the aa05 reply look like?
print("\n=== AA05 REPLIES (performance) ===")
result2 = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields',
     '-e', 'frame.number',
     '-e', 'usb.capdata',
     '-e', 'usbhid.data',
     '-Y', 'usb.data_len > 0'],
    capture_output=True, text=True, cwd='.')

for line in result2.stdout.strip().split('\n'):
    parts = line.split('\t')
    if len(parts) < 3:
        continue
    frame = int(parts[0])
    data = parts[1] or parts[2]
    if not data:
        continue
    clean = data.replace(':', '').replace(' ', '').lower()
    if clean[:4] == 'aa05' and 460 <= frame <= 500:
        raw = bytes.fromhex(clean)
        print(f"\nframe {frame}: aa05 reply ({len(raw)} bytes)")
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# Also: for a SECOND effect (effect 7), check if zone changes
print("\n=== ZONE FOR EFFECT 7 (frame 537) ===")
for frame, hx in sends:
    if frame == 537:
        raw = bytes.fromhex(hx)
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# And zone for effect 1 (frame 713)
print("\n=== ZONE FOR EFFECT 1 (frame 713) ===")
for frame, hx in sends:
    if frame == 713:
        raw = bytes.fromhex(hx)
        for i in range(0, len(raw), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")

# Check the 55 08 packets - what are they?
print("\n=== 55 08 PACKETS (unknown cmd) ===")
for frame, hx in sends:
    if hx[2:4] == '08' and frame < 500:
        raw = bytes.fromhex(hx)
        print(f"frame {frame}: ({len(raw)} bytes)")
        for i in range(0, min(len(raw), 32), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")
        print()

# Check the 55 a0 packets
print("=== 55 a0 PACKETS ===")
for frame, hx in sends:
    if hx[2:4] == 'a0' and frame < 500:
        raw = bytes.fromhex(hx)
        print(f"frame {frame}: ({len(raw)} bytes)")
        for i in range(0, min(len(raw), 32), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")
        print()

# 55 f1 packets (what are these?)
print("=== 55 f1 PACKETS (before first effect flow) ===")
for frame, hx in sends:
    if hx[2:4] == 'f1' and frame < 200:
        raw = bytes.fromhex(hx)
        print(f"frame {frame}: ({len(raw)} bytes)")
        for i in range(0, min(len(raw), 32), 16):
            chunk = raw[i:i+16]
            print(f"  [{i:3d}] {' '.join(f'{b:02x}' for b in chunk)}")
        print()
