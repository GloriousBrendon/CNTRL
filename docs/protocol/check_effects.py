#!/usr/bin/env python3
import json

with open('src-tauri/resources/effects_light_v2.json') as f:
    data = json.load(f)

print("=== EFFECT TEMPLATE CHECKSUM VERIFICATION ===")
all_good = True
for k in sorted(data['templates'].keys(), key=int):
    hx = data['templates'][k]
    raw = bytes.fromhex(hx)
    actual_b3 = raw[3]
    computed = sum(raw[4:]) & 0xFF
    ok = actual_b3 == computed
    if not ok:
        all_good = False
    eid = raw[21]
    print(f"  effect {k:>2}: b3={actual_b3:02x} computed={computed:02x} {'OK' if ok else 'BAD'}  effect_byte[21]={eid:02x} ({eid})")

if all_good:
    print("\nAll effect templates have CORRECT checksums already!")
else:
    print("\nSome templates have BAD checksums - need fixing!")

# Also check: what zone packet does the official app send for effects?
# And what apply_a/b does it use?
print("\n=== EFFECT PIPELINE: does it use ZONE + APPLY like solid? ===")
# Check from pcap - the 55 06 packets near effect writes
# From the grep above, we see 55 06 packets after 55 f2 effect writes
# Let me verify the zone packet pattern for effects
print("From pcap grep: effects use same pipeline: WRITE_LIGHT(55f2) -> ZONE(55f2) -> APPLY_A(5506) -> APPLY_B(5506)")
