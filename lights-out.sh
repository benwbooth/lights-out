#!/usr/bin/env bash
# Turn off all RGB LEDs on the system

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LEDCTL="$SCRIPT_DIR/lights-out/target/release/lights-out"

echo "=== Turning off RGB lights ==="
echo ""

# Custom devices (MSI cooler, LianLi fans, GPU)
echo "[lights-out] Custom devices..."
if [[ -x "$LEDCTL" ]]; then
    sudo "$LEDCTL" off || echo "  Warning: lights-out off failed; continuing"
else
    echo "  Warning: lights-out not found at $LEDCTL"
    echo "  Build with: cd $SCRIPT_DIR/lights-out && nix develop --command cargo build --release"
fi

echo ""
echo "[OpenRGB] Detected devices..."

# Address devices by name — indices shift when things like the DualSense
# controller connect/disconnect.

# A device that is unplugged or asleep (the mouse dongle, for example) must not
# abort the script: under `set -e` that left later devices lit and failed the
# boot service.
openrgb_off() {
    local label="$1"
    shift
    echo "  $label"
    if ! openrgb "$@"; then
        echo "  Warning: could not set $label (device missing?); continuing"
    fi
}

# At boot this runs right after openrgb.service starts, while the server is
# still detecting hardware: it may list only some devices, and commands sent
# then get lost. Wait until the devices that are always installed (both RAM
# sticks and the motherboard) are listed. The mouse is left out because its
# dongle can be asleep or unplugged.
RAM_NAME="Corsair Dominator Platinum RGB DDR5"
BOARD_NAME="ASRock X670E Taichi"
DETECT_TIMEOUT=90

list_devices() {
    openrgb --list-devices 2>/dev/null | grep -E '^[0-9]+: ' || true
}

ram_indices() {
    list_devices | grep -F ": $RAM_NAME" | cut -d: -f1
}

deadline=$((SECONDS + DETECT_TIMEOUT))
until [[ $(ram_indices | wc -l) -ge 2 ]] && list_devices | grep -qF ": $BOARD_NAME"; do
    if ((SECONDS >= deadline)); then
        echo "  Warning: OpenRGB still missing devices after ${DETECT_TIMEOUT}s; continuing with what it has"
        break
    fi
    sleep 2
done

# Both sticks share a name, so address them by the indices the server
# reports for that name.
stick=0
for index in $(ram_indices); do
    stick=$((stick + 1))
    openrgb_off "Corsair RAM (stick $stick) -> black" --device "$index" --mode Direct --color 000000
done
if ((stick == 0)); then
    echo "  Warning: no Corsair RAM found in OpenRGB; continuing"
fi

openrgb_off "ASUS ROG Chakram X mouse -> black" --device "Asus ROG Chakram X 2.4GHz" --mode Direct --color 000000

echo "  Red Square Keyrox keyboard -> (use Fn+Del manually)"

openrgb_off "ASRock X670E Taichi motherboard -> off" --device "$BOARD_NAME" --mode Off

echo ""
echo "=== Summary ==="
echo "Controlled by lights-out:"
echo "   - MSI MPG CORELIQUID cooler (LEDs + LCD)"
echo "   - LianLi UNI FAN-AL V2 fans"
echo "   - ASUS TUF Gaming RX 7900 XTX GPU"
echo ""
echo "Controlled by OpenRGB:"
echo "   - Corsair Dominator Platinum RAM (x2)"
echo "   - ASUS ROG Chakram X mouse"
echo "   - ASRock X670E Taichi motherboard"
echo ""
echo "Manual control required:"
echo "   - Red Square Keyrox keyboard: Fn+Del"
echo "   - Kinesis Freestyle Edge RGB: LED key toggle"
echo ""
echo "Done!"
