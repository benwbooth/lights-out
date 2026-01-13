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

# Device 0: Corsair Dominator Platinum (RAM stick 1) - set to black
echo "  [0] Corsair RAM (stick 1) -> black"
openrgb --device 0 --mode Direct --color 000000

# Device 1: Corsair Dominator Platinum (RAM stick 2) - set to black
echo "  [1] Corsair RAM (stick 2) -> black"
openrgb --device 1 --mode Direct --color 000000

# Device 2: Asus ROG Chakram X (Mouse) - set to black
echo "  [2] ASUS ROG Chakram X mouse -> black"
openrgb --device 2 --mode Direct --color 000000

# Device 3: Red Square Keyrox TKL Classic (Keyboard)
# NOTE: OpenRGB can't fully control this keyboard's backlight
echo "  [3] Red Square Keyrox keyboard -> (use Fn+Del manually)"

# Device 4: ASRock X670E Taichi (Motherboard) - has Off mode
echo "  [4] ASRock X670E Taichi motherboard -> off"
openrgb --device 4 --mode Off

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
