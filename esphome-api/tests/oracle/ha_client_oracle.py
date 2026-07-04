"""Conformance oracle for the native-API connection FSM.

Drives the REAL Home Assistant client (aioesphomeapi) against our Rust device
over a loopback socket and asserts VALUES, not mere completion: the device_info
fields, the exact entity list, and that at least two DISTINCT sensor states are
observed. Prints a JSON summary and exits 0 on success, non-zero on any
mismatch — the Rust harness (tests/aioesphomeapi_oracle.rs) reads that verdict.

Usage: python ha_client_oracle.py <port>
"""
import asyncio
import json
import sys

from aioesphomeapi import APIClient


async def run(port: int) -> dict:
    client = APIClient("127.0.0.1", port, None)  # no password -> plaintext
    await client.connect(login=False)

    device = await client.device_info()
    entities, _services = await client.list_entities_services()

    distinct_states: set[float] = set()
    saw_two = asyncio.Event()

    def on_state(state) -> None:
        value = getattr(state, "state", None)
        if value is not None:
            distinct_states.add(round(float(value), 4))
        if len(distinct_states) >= 2:
            saw_two.set()

    client.subscribe_states(on_state)
    try:
        await asyncio.wait_for(saw_two.wait(), timeout=10.0)
    finally:
        await client.disconnect()

    # --- Assertions: the oracle checks values, so a green means HA really adopts
    #     this device and reads it, not just that the sockets connected. The full
    #     sensor descriptor is asserted — name, unit, class, precision — because HA
    #     renders and aggregates the entity from exactly these fields. ---
    assert device.name == "plantmon", f"device name was {device.name!r}"
    assert device.mac_address.lower() == "aa:bb:cc:dd:ee:ff", f"mac was {device.mac_address!r}"
    assert len(entities) == 1, f"expected exactly one entity, got {len(entities)}"
    entity = entities[0]
    assert entity.object_id == "soil_moisture", f"object_id was {entity.object_id!r}"
    assert entity.name == "Soil Moisture", f"entity name was {entity.name!r}"
    assert entity.unit_of_measurement == "%", f"unit was {entity.unit_of_measurement!r}"
    assert entity.device_class == "moisture", f"device_class was {entity.device_class!r}"
    assert entity.accuracy_decimals == 0, f"accuracy_decimals was {entity.accuracy_decimals!r}"
    # state_class is an IntEnum; 1 == MEASUREMENT (how HA aggregates history).
    assert int(entity.state_class) == 1, f"state_class was {int(entity.state_class)}"
    assert len(distinct_states) >= 2, f"expected >=2 distinct states, got {sorted(distinct_states)}"

    return {
        "device_name": device.name,
        "mac_address": device.mac_address,
        "entity_count": len(entities),
        "object_id": entity.object_id,
        "entity_name": entity.name,
        "unit_of_measurement": entity.unit_of_measurement,
        "device_class": entity.device_class,
        "accuracy_decimals": entity.accuracy_decimals,
        "state_class": int(entity.state_class),
        "distinct_states": sorted(distinct_states),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: ha_client_oracle.py <port>", file=sys.stderr)
        return 2
    port = int(sys.argv[1])
    summary = asyncio.run(run(port))
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
