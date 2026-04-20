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
    sudo "$LEDCTL" off
else
    echo "  Warning: lights-out not found at $LEDCTL"
    echo "  Build with: cd $SCRIPT_DIR/lights-out && nix develop --command cargo build --release"
fi

echo ""
echo "[OpenRGB] Detected devices..."

# Address devices by name — indices shift when things like the DualSense
# controller connect/disconnect.

echo "  Corsair RAM (stick 1) -> black"
openrgb --device 0 --mode Direct --color 000000

echo "  Corsair RAM (stick 2) -> black"
openrgb --device 1 --mode Direct --color 000000

echo "  ASUS ROG Chakram X mouse -> black"
openrgb --device "Asus ROG Chakram X 2.4GHz" --mode Direct --color 000000

echo "  Red Square Keyrox keyboard -> (use Fn+Del manually)"

echo "  ASRock X670E Taichi motherboard -> off"
openrgb --device "ASRock X670E Taichi" --mode Off

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
