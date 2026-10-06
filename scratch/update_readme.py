import re

with open('README.md', 'r') as f:
    content = f.read()

# Replace title
content = content.replace('# π RuView', '# π WiiViewOnlyReal')

# Insert differences
diff_text = """

## ⚠️ Fork Notice: Differences from upstream RuView
**WiiViewOnlyReal** is a hard fork of RuView focusing strictly on physical hardware operation, stability, and room-level intelligence.

Key differences from the original `ruvnet/RuView`:
- **Hardware Only**: Simulation and fallback demo data generation have been completely disabled. It enforces real ESP32 CSI parsing and strictly runs on actual physical hardware data.
- **Room-Level Vitals Fusion**: Instead of resolving vitals node-by-node, multiple ESP32 nodes are bound into logical "Rooms". Vitals (Heart rate, Breathing) are fused and cross-checked among all nodes in a room to produce a single, high-confidence biological reading.
- **Improved UI & Calibration**: Features a modified Observatory and Sensing UI with a drop-down Room Selector, allowing you to easily manage and recalibrate entire rooms at once. Unlimited calibration frame captures are supported.
- **Standalone AppImage**: A new distribution method allowing users to run the server, UI, and access firmware binaries from a single, portable `x86_64` AppImage.

"""

content = content.replace('## **See through walls with WiFi** ##', diff_text + '## **See through walls with WiFi** ##')

# Replace some other instances of RuView to WiiViewOnlyReal where appropriate
content = content.replace('### π RuView is a WiFi sensing platform', '### π WiiViewOnlyReal is a WiFi sensing platform')

with open('README.md', 'w') as f:
    f.write(content)
