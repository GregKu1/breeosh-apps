# Goods In — Tauri port: context

Bakery goods-receiving app, ported from a Python-server + single-page-HTML app
into a standalone Tauri v2 desktop app. This file exists so a fresh Claude
Code session (on this machine or another) can pick up where things left off.

## Where things are

- **Repo root**: this directory (`goods_entry_system/`). Git remote:
  `git@github.com:GregKu1/breeosh-goods-in-system.git`, branch `main`.
- **`goods-in-tauri/`** — the ported Tauri app. This is the thing to open,
  build, and run going forward.
- **`goods-in-package/`** (extracted from `goods-in-package.zip`) — the
  *original* pre-port app: `goods-in.html` + `server.py`. Kept for reference
  only. **Not committed to git** (untracked) — if you want it preserved on
  another machine, `git add` it explicitly or copy the folder by hand.
- **`tauri-port-prompt.md`** — the original porting spec that was followed.
  Also untracked.
- As of the last commit (`1a5ff62 "latest manual"`), local `main` and
  `origin/main` are identical — everything in `goods-in-tauri/` is pushed.
  Cloning the repo fresh gets you the current state.

## What the Tauri app looks like

- `src-tauri/src/lib.rs` — Rust backend. Two commands: `read_products` /
  `write_products`. `products.json` lives next to the running executable
  (via `std::env::current_exe()`), not in an OS app-data path — atomic
  tmp-file-then-rename write, corrupt file renamed to `.broken`. Mirrors the
  old `server.py` behaviour exactly.
- `src/index.html` — the original `goods-in.html`, frontend logic otherwise
  untouched except:
  - `loadDbFromServer()` / `saveDbToServer()` now call
    `invoke('read_products')` / `invoke('write_products', {data})` instead of
    `fetch('/api/products')`.
  - JsBarcode vendored locally at `src/vendor/jsbarcode.all.min.js` (no CDN
    dependency).
- `src-tauri/tauri.conf.json` — window title "Goods In", 1100×780. CSP allows
  `connect-src`/`img-src` for `world.openfoodfacts.org` (the barcode lookup
  API) since that's the one external network call the app makes.

### One correction to the original porting prompt

The prompt said to use `window.__TAURI__.tauri.invoke` — that's the **Tauri
v1** namespace. This is Tauri **v2**, where the global is
`window.__TAURI__.core.invoke`. Confirmed against the actual
`create-tauri-app` v2 template output and used that instead.

## Features added on top of the straight port

1. **Edit a product in the database** (Database tab → "Edit" button per
   row). Modal lets you change name/SKU/unit/brand/supplier/grams-per-unit/
   size-per-lot for an existing EAN entry, then saves via `write_products`.
2. **Remembered "size per lot"** — added `lotSize` to the product-database
   schema (alongside the existing `gramsPerUnit`). Saved automatically
   whenever a receiving line is added, and pre-filled next time that barcode
   is scanned/searched — same pattern as the existing grams-per-unit
   conversion memory.
3. **Reprint a label from the database** (Database tab → "Print label"
   button per row). Opens a modal to hand-enter batch/lot + BBE + copies +
   label size, for reprinting a sticker that's no longer in today's Outputs
   list. Refactored the Outputs-tab print handler into a shared
   `printExpandedLabels()` function so both flows use identical label/
   barcode-rendering logic.
4. **Product database is keyed by EAN, not SKU — duplicate SKUs across
   different EANs are allowed on purpose.** Confirmed this is correct as-is:
   the user wants one Craftybase "material" (SKU) to be able to come in from
   multiple barcodes (different pack sizes / suppliers), while each EAN
   keeps its own name/brand/supplier/lot-size, and each Craftybase CSV row
   is still built independently per receiving line. No enforcement was
   added — this was a deliberate design confirmation, not an oversight.

## UI tweaks made

- `.lookup-result` (the grey "known item" box under "2. Confirm the
  product"): `margin-top: 4px; margin-bottom: 16px;` — was sitting right up
  against the heading above and the Internal SKU field below; both were
  adjusted per feedback.
- Label size `<select>` options (both the Outputs-tab `#labelSizeSelect` and
  the reprint modal's `#rpLabelSize`) are ordered by Dymo part number:
  99010 (89×28mm) → 99012 (89×36mm) → 99014 (101×54mm).

## Build target reminder

- **The bakery PC is 32-bit Windows** (Intel Atom, 4 core, 2GB RAM, 32GB disk).
  Build with `npm run tauri build -- --target i686-pc-windows-msvc`
  (needs `rustup target add i686-pc-windows-msvc` once). An x86_64 build fails
  to launch there with "This app can't run on your PC" — the PE loader can't
  load a 64-bit binary on 32-bit Windows. A 32-bit build also runs fine on
  64-bit Windows via WoW64, so i686 is the safe single target to ship.
  Build **release**, not `--debug`, given the 2GB RAM.
  (An earlier version of this file assumed x86_64. That was never verified
  against the real machine and was wrong — hence this note.)
- Development has since moved to a Windows machine; `.msi` and NSIS
  `-setup.exe` bundles now build locally for both x86_64 and i686.
- Two things to watch when deploying to that PC:
  - **WebView2 x86 runtime.** 32-bit Windows means Windows 10 or older
    (Windows 11 has no 32-bit edition), and only Windows 11 preinstalls
    WebView2 — so the runtime is probably absent. The installer's default
    `webviewInstallMode` downloads it at install time, which needs internet on
    that PC. Switch to `offlineInstaller` in `tauri.conf.json` if it has none.
  - **`products.json` is written next to the `.exe`** (see `lib.rs`). If the
    app is *installed* into `C:\Program Files\`, that directory isn't
    writable by a standard user and saving the product database may fail.
    Running it from a normal folder (e.g. `C:\GoodsIn\`) sidesteps this and
    matches the "database sits next to the app" design.

## Not done / possibly worth revisiting

- localStorage → disk persistence for `gi_lines_v1` / `gi_active_invoice_v1`
  was explicitly deprioritized in the original port prompt ("acceptable for
  v1") and hasn't been revisited.
- No actual Windows build/run has happened yet — first priority when a
  Windows machine/runner is available.
- No automated tests exist; everything has been verified by compiling +
  launching the dev build and (for the two feature additions) reading the
  code paths carefully rather than clicking through the UI end-to-end.
