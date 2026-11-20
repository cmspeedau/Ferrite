# Ferrite Theme (Satisfactory-inspired)

Design reference: `docs/dock-mockup.html`

## Palette

| Role | Value |
|---|---|
| Primary accent | `#ff6b00` |
| Accent gradient | `#ff8533 → #ff6b00 → #cc5500` |
| Base background | `#08090b` |
| Surface (dark) | `#17181d` |
| Surface (light) | `#282c34` |
| Text | `#eceff3` |
| Border (hairline) | `rgba(255,255,255,0.08)` |
| Hover/active glow | orange `box-shadow`, low alpha |

## Usage notes

- The accent orange is used for active indicators, focus borders, hover glows, and the hex/launcher button.
- Backgrounds are near-black with a slight blue-grey tint.
- When implementing in Rust (layer-shell panel), map these to RGBA values for the renderer.
