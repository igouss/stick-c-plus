"""Capture golden plaintext native-API frames from aioesphomeapi itself.

The payload is serialized by aioesphomeapi's vendored `api_pb2` (the Python
protobuf), and the frame wrapper is produced by aioesphomeapi's own
`make_plain_text_packets`. Both are the REAL client's code paths, so the bytes
this emits are an independent oracle for our Rust codec: encoding must be
byte-exact against them, and decoding must recover the same type + payload.

Emits a TSV: name<TAB>msg_type<TAB>frame_hex<TAB>payload_hex<TAB>note
"""
import sys
from aioesphomeapi import api_pb2 as pb
from aioesphomeapi._frame_helper.packets import make_plain_text_packets


def frame(msg_type: int, payload: bytes) -> bytes:
    parts = make_plain_text_packets([(msg_type, payload)])
    return b"".join(parts)


cases = []

# A: PingRequest — empty payload, type 7. Frame must be exactly 00 00 07.
ping = pb.PingRequest()
cases.append(("PingRequest", 7, ping.SerializeToString(),
              "empty payload; frame is preamble+len(0)+type only"))

# B: HelloResponse — a few scalar + string fields, type 2.
hello = pb.HelloResponse()
hello.api_version_major = 1
hello.api_version_minor = 14
hello.server_info = "esphome-api"
hello.name = "plantmon"
cases.append(("HelloResponse", 2, hello.SerializeToString(),
              "mixed varint + string fields"))

# C: SensorStateResponse — fixed32 key + float state, type 25.
st = pb.SensorStateResponse()
st.key = 0x1A2B3C4D
st.state = 42.5
st.missing_state = False  # proto3 default -> omitted by both impls
cases.append(("SensorStateResponse", 25, st.SerializeToString(),
              "fixed32 key + IEEE-754 float; default bool omitted"))

# D: DeviceInfoResponse with a long model string -> payload >= 128 bytes, so the
#    length varuint is TWO bytes. Exercises the varint length boundary.
dev = pb.DeviceInfoResponse()
dev.name = "plantmon"
dev.model = "M" * 200
dev.esphome_version = "rust-0.1"
payload_d = dev.SerializeToString()
assert len(payload_d) >= 128, f"want a multi-byte length varint, got {len(payload_d)}"
cases.append(("DeviceInfoResponse", 10, payload_d,
              "payload >=128 bytes: two-byte length varuint"))

# E: ListEntitiesSensorResponse — a List message, type 16.
le = pb.ListEntitiesSensorResponse()
le.object_id = "soil_moisture"
le.key = 0x1A2B3C4D
le.name = "Soil Moisture"
le.unit_of_measurement = "%"
le.accuracy_decimals = 0
le.device_class = "moisture"
cases.append(("ListEntitiesSensorResponse", 16, le.SerializeToString(),
              "entity List message"))

out = sys.stdout
for name, mtype, payload, note in cases:
    fr = frame(mtype, payload)
    out.write(f"{name}\t{mtype}\t{fr.hex()}\t{payload.hex()}\t{note}\n")
