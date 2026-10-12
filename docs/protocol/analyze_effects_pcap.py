#!/usr/bin/env python3
"""Extract HID payloads from the mchose_all_effects pcap and analyze the effect pipeline."""
import subprocess, re, sys

# Extract all USB HID data from pcap using tshark
# We want the raw HID payload on interface 1 (the lighting control interface)
result = subprocess.run(
    ['tshark', '-r', 'mchose_all_effects.pcap',
     '-T', 'fields',
     '-e', 'frame.number',
     '-e', 'usb.endpoint_address',
     '-e', 'usb.transfer_type',
     '-e', 'usb.data_len',
     '-e', 'usb.capdata',
     '-e', 'usbhid.data',
     '-Y', 'usb.data_len > 0'],
    capture_output=True, text=True, cwd='.')

if result.returncode != 0:
    print("tshark error:", result.stderr)
    sys.exit(1)

lines = result.stdout.strip().split('\n')
print(f"Total packets with data: {len(lines)}")

# Parse out 0x55-prefixed HID packets (host->device sends)
sends = []  # (frame_no, hex_payload)
recvs = []

for line in lines:
    parts = line.split('\t')
    if len(parts) < 6:
        continue
    frame = parts[0]
    endpoint = parts[1]
    data = parts[4] or parts[5]  # capdata or hid data
    if not data:
        continue
    # Clean the hex
    clean = data.replace(':', '').replace(' ', '').lower()
    if len(clean) < 4:
        continue
    
    # Identify sends (host to device = endpoint OUT, usually 0x04 or endpoint_address bit 7 = 0)
    # 0x55 prefix = host->device command
    if clean[:2] == '55':
        sends.append((int(frame), clean))
    elif clean[:2] == 'aa':
        recvs.append((int(frame), clean))

print(f"\nHost->device (0x55) sends: {len(sends)}")
print(f"Device->host (0xAA) recvs: {len(recvs)}")

# Group sends by type
by_type = {}
for frame, hx in sends:
    cmd = hx[2:4]  # byte[1]
    by_type.setdefault(cmd, []).append((frame, hx))

print("\n=== SEND COMMAND TYPES ===")
for cmd, pkts in sorted(by_type.items()):
    print(f"  cmd=0x{cmd}: {len(pkts)} packets")

# Focus on the 55 f2 writes (WRITE_LIGHT_V2 and ZONE) 
print("\n=== ALL 55 f2 PACKETS (profile + zone) ===")
f2_pkts = by_type.get('f2', [])
for frame, hx in f2_pkts:
    raw = bytes.fromhex(hx)
    b3 = raw[3]
    b4 = raw[4]
    b5 = raw[5]
    b18 = raw[18] if len(raw) > 18 else 0
    b21 = raw[21] if len(raw) > 21 else 0
    b22 = raw[22] if len(raw) > 22 else 0
    b23 = raw[23] if len(raw) > 23 else 0
    b24 = raw[24] if len(raw) > 24 else 0
    
    # Is it a zone packet? Zone packets have byte[5] = 0x07 or 0x0d etc
    is_zone = (b4 == 0x38 and b5 in (0x07, 0x01, 0x0d))
    is_profile = (b4 == 0x38 and b5 == 0xd8)
    
    checksum_ok = b3 == (sum(raw[4:]) & 0xFF)
    
    if is_profile:
        print(f"  frame {frame:5}: PROFILE  effect_byte={b21:02x}({b21:2d})  b18={b18:02x}  RGB={b22:02x}{b23:02x}{b24:02x}  cksum={'OK' if checksum_ok else 'BAD'}")
    elif is_zone:
        print(f"  frame {frame:5}: ZONE     b5={b5:02x}  b6={raw[6]:02x}  b7={raw[7]:02x}  b8={raw[8]:02x}  cksum={'OK' if checksum_ok else 'BAD'}")
        print(f"             full: {hx[:40]}...")
    else:
        print(f"  frame {frame:5}: OTHER  b4={b4:02x} b5={b5:02x}   cksum={'OK' if checksum_ok else 'BAD'}")
        print(f"             full: {hx[:40]}...")

# Focus on 55 06 (APPLY)
print("\n=== ALL 55 06 PACKETS (APPLY A/B) ===")
a06_pkts = by_type.get('06', [])
for frame, hx in a06_pkts:
    raw = bytes.fromhex(hx)
    b3 = raw[3]
    b9 = raw[9] if len(raw) > 9 else 0
    checksum_ok = b3 == (sum(raw[4:]) & 0xFF)
    print(f"  frame {frame:5}: b4-b7={raw[4]:02x}{raw[5]:02x}{raw[6]:02x}{raw[7]:02x}  b8-b11={raw[8]:02x}{raw[9]:02x}{raw[10]:02x}{raw[11]:02x}  cksum={'OK' if checksum_ok else 'BAD'}")
    # Show RGB region
    print(f"             b14-16={raw[14]:02x}{raw[15]:02x}{raw[16]:02x}  b22-24={raw[22]:02x}{raw[23]:02x}{raw[24]:02x}")

# Also show the 55 04 and 55 05 packets
print("\n=== 55 04 (GET_DIY_LIGHT) ===")
for frame, hx in by_type.get('04', []):
    print(f"  frame {frame}")

print("\n=== 55 05 (GET_PERFORMANCE) ===")
for frame, hx in by_type.get('05', []):
    raw = bytes.fromhex(hx)
    print(f"  frame {frame}: b4={raw[4]:02x} b5={raw[5]:02x}")
