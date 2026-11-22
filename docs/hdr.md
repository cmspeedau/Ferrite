# HDR Support — Feasibility & Roadmap

Status: **Not implemented.** Research note; tracked as a roadmap item in `README.md`.

## What Ferrite already has

- Renderer: `GlowRenderer` (GLES). `SUPPORTED_FORMATS` in
  `src/backends/udev.rs` already lists 10-bit scanout formats
  (`Abgr2101010`, `Argb2101010`) alongside 8-bit ones, so the
  allocator/compositor path can in principle scan out 10-bit buffers.
- `GlesRenderer` exposes a `ColorTransformations` capability (half-float
  shadow buffer, `RGBA16F`) when `GLES 3.0 + GL_EXT_color_buffer_half_float`
  are available. That is the prerequisite for HDR blending, but Ferrite does
  not currently request or use it for tone mapping.
- DRM side uses `DrmCompositor` with `GbmAllocator`; no HDR metadata is set.

## Upstream status (Smithay PR #1143)

Still a **draft** as of Nov 2026, despite the protocols (`wp-color-management-v1`) now being
released/stable. The maintainer (Drakulix) states the protocol side is the easy part; the
rendering-pipeline (color-space-aware blending, tone mapping, CTM/gamma LUTs) has not been
started, and the PR itself is untested. A community fork
([dividebysandwich/smithay `ds-hdr-master`](https://github.com/dividebysandwich/smithay/tree/ds-hdr-master))
has working windowed/fullscreen HDR passthrough on NVIDIA and AMD, built on this PR's foundations.

Conclusion: HDR is blocked on upstream Smithay. Ferrite cannot implement it independently without
forking Smithay and carrying the color-management pipeline ourselves. Track this PR and the
community fork; revisit when Smithay merges color management.

## What full HDR requires

1. **DRM output**: set `DRM_CONNECTOR_HDR_OUTPUT_METADATA` on the connector,
   enable the CRTC color-management pipeline (CTM / gamma LUTs / degamma),
   scan out 10-bit or FP16 buffers, and handle per-output HDR toggle.
2. **Renderer**: half-float intermediate, tone mapping (PQ / HLG), and
   color-space conversion (BT.2020 ↔ display). Smithay provides the half-float
   shadow buffer capability but no tone-mapping shaders.
3. **Wayland protocol**: `wp-color-management-v1` and
   `wp-color-representation-v1` so clients can declare transfer function and
   color space per surface, and the compositor can advertise supported
   combinations.
4. **Per-surface handling**: convert each client buffer according to its
   declared color metadata before compositing.
5. **Config / UX**: per-output HDR toggle, color profile, and (optionally)
   auto-detect from EDID.

## Blockers (current)

- Ferrite pins Smithay at git `c293ec7` (0.3.0 line). That revision has **no**
  color-management protocol states, **no** DRM HDR-metadata API, and **no**
  tone-mapping in the renderer.
- Smithay 0.7.0 (newest in the local registry) also lacks all of the above.
- The GLES renderer explicitly avoids color transformations on ES 2.0; HDR
  needs GLES 3.0 + `GL_EXT_color_buffer_half_float` on the GPU/driver.
- DRM drivers vary: AMD and recent Intel (i915) support HDR metadata well;
  NVIDIA's proprietary stack has gaps.

## Phased plan

- **Phase 0 (done):** document feasibility, add roadmap item. Keep 10-bit
  formats listed but treat them as SDR-wide-gamut only.
- **Phase 1:** track Smithay upstream for color-management support; bump the
  pin once a release exposes the needed APIs. No Ferrite code until then.
- **Phase 2:** implement `wp-color-management-v1` state in Ferrite, wire
  surface color info into the render path.
- **Phase 3:** DRM HDR metadata + tone-mapping shaders in the GL renderer
  (half-float pipeline).
- **Phase 4:** config keys + per-output toggle + detection from EDID.

## Recommendation

Do not start implementation now. The cost is high and the dependency (Smithay)
does not yet expose the required primitives. Revisit when Smithay ships
color-management support; until then, the 10-bit format entries can stay as
forward-compatible placeholders.
