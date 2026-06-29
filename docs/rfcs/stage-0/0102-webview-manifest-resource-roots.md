<!-- exo:102 ulid:01kg5kp2g20rpn6g52p522796h -->

# RFC 102: Webview Manifest for Resource Roots


# RFC 0102: Webview Manifest for Resource Roots

## Summary

Use a build-generated webview manifest to determine asset locations and automatically set `localResourceRoots` and HTML asset references for Studio and other webviews.

## Motivation

Studio webviews render blank if local assets (e.g., `studio.js`, `studio.css`) are blocked by missing `localResourceRoots`. Since assets are already produced by Vite, we should derive allowed roots and asset URIs from a manifest instead of hardcoding values.

## Detailed Design

### Terminology

- **Webview Manifest**: A JSON file produced at build time that enumerates webview entrypoints and their emitted assets.
- **Asset Roots**: The directories containing the manifest’s JS/CSS assets.

### User Experience (UX)

- Studio and other webviews render reliably without manual updates when asset paths change.
- No user configuration required.

### Architecture

- Vite outputs `out/webview/manifest.json` with an entry per webview bundle.
- A shared helper reads the manifest and constructs:
  - `localResourceRoots` for the webview
  - `scriptUri`/`styleUri` for HTML
- All webviews use the helper; no bespoke asset wiring.

### Implementation Details

1. **Build Step**
   - Enable `build.manifest = true` in the webview Vite config (if not already).
   - Ensure the manifest is copied to `out/webview/manifest.json`.

2. **Helper API**
   - `getWebviewAssets(webview, extensionUri, entryName)` returns:
     - `localResourceRoots`
     - `scriptUri` / `styleUri` (or arrays)

3. **Webview Initialization**
   - Every webview uses the helper to set `webview.options.localResourceRoots`.
   - HTML uses manifest-derived URIs only.

## Implementation Plan (Stage 2)

- [ ] Add manifest generation to webview build.
- [ ] Create a shared helper for manifest parsing and webview setup.
- [ ] Migrate Studio and Dashboard to the helper.
- [ ] Add tests asserting webviews use the helper (not raw paths).

## Context Updates (Stage 3)

- [ ] Update `docs/manual/architecture/webviews.md` (or equivalent) with manifest-based assets.

## Drawbacks

- Requires manifest parsing at runtime.
- Slightly more complex build pipeline.

## Alternatives

- Continue hardcoding `localResourceRoots` and asset paths (fragile).
- Add a static list of allowed roots (still manual).

## Unresolved Questions

- Should the helper cache the manifest or read per webview?
- How to handle multiple entrypoints with shared chunks?

## Future Possibilities

- Centralized asset preloading.
- Automatic CSP generation based on manifest assets.

