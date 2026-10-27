# Startup logo

Put the Brass Knuckle Gang logo here as `logo.png`, then run:

    python3 tools/spritegen.py

The script fits the logo into 76 x 40 art pixels (152 x 80 on screen), keeps its transparent
background transparent, and cuts it down to 15 colors. Without `logo.png`, the startup screen
shows a drawn pair of brass knuckles instead. Any 8-bit PNG works: RGB, RGBA, grayscale or
indexed, not interlaced. A logo on a transparent background looks best.
