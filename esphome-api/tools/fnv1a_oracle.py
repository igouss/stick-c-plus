"""Independent FNV-1a (32-bit) oracle for entity keys.

ESPHome derives an entity's native-API `key` (a fixed32) from a deterministic,
seedless hash of its object_id, so keys are stable across reboots and HA never
churns entities. This computes the same hash in a second language over the
object_id's UTF-8 bytes, giving the Rust `entity::object_id_key` a cross-impl
oracle (cf. the crate's two-oracle id test). Emits: object_id<TAB>key_hex
"""
OFFSET = 0x811C9DC5   # 2166136261
PRIME = 0x01000193    # 16777619
MASK = 0xFFFFFFFF

def fnv1a(data: bytes) -> int:
    h = OFFSET
    for b in data:
        h = ((h ^ b) * PRIME) & MASK
    return h

cases = [
    "soil_moisture",
    "temperature",
    "living_room_light",
    "",                 # empty object_id -> the bare offset basis
    "x",                # single byte
    "M" * 100,          # long
    "café",        # non-ASCII: proves the hash is over UTF-8 BYTES, not chars
]
for oid in cases:
    print(f"{oid}\t{fnv1a(oid.encode('utf-8')):08x}")
