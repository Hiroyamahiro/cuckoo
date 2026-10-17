# Third-party notices

The user-facing document is src/public/THIRD-PARTY-NOTICES.html. It is a
self-contained UTF-8 HTML file: no JavaScript, external styles, fonts, or network
requests are needed to read any notice. Vite copies it unchanged into dist/;
Tauri embeds it in the release executable. The existing credits dialog opens
this local document in its dedicated window. It can also be opened directly in
a browser without Cuckoo.

## Reproduction

Use Windows x64, the locked Node/Rust dependencies, and the normal release
features (no verification harness). Run npm ci and cargo fetch --locked
--manifest-path src-tauri/Cargo.toml when initially provisioning dependencies.
Then run:

    npm run licenses:generate
    npm run licenses:check
    npm run build
    npm test
    npm run build:installer

Generation uses only locally fetched inputs. It never installs tools or fetches
license text from mutable web pages. npm run build checks for stale output and
fails before bundling if regeneration is needed. Keep the generated document in
Git for review, together with the generator and supplemental.json. Do not commit
Cargo caches, browser profiles, old snapshots, binaries, or investigation output;
those belong in ignored test-runtime/ or existing ignored build directories.

## Selection and provenance

scripts/generate-notices.mjs uses cargo tree with --locked --offline,
--target x86_64-pc-windows-msvc, --features tauri/custom-protocol,
--edges normal,no-proc-macro and --no-dedupe. This is the release protocol
feature used by Tauri packaging. On this revision it yields 180 third-party Rust
package/version pairs, including transitive dependencies. The default graph has
the same 180 pairs. Cargo metadata supplies the corresponding source directories,
license declarations, authors, and exact versions; its feature-unified edges are
not used alone for dependency selection.

All discovered LICENSE/LICENCE/COPYING/NOTICE/COPYRIGHT/AUTHORS files, license
directories, declared license-file entries, and copyright comments in source
files are retained. Build/test/example directories are not harvested as runtime
source. No standalone NOTICE-named file was found in the selected crates in
this revision, but the generator includes such files automatically when present.
The declarations are retained verbatim; alternatives are not silently rewritten.
Both alternatives' supplied texts may be retained conservatively.

supplemental.json holds the existing non-Rust attribution and exact-version
exceptions for published crates that omit workspace license files:
alloc-stdlib 0.3.0, dasp_sample 0.11.0, webview2-com 0.39.1,
webview2-com-sys 0.39.1, and the four Symphonia 0.5.5 crates. Exceptions verify
the declared license; a new version without license files fails generation until
reviewed. Complete Apache and MPL texts supplement short notices. The exact
source archive URL for each Rust component is retained, with the MPL source
availability statement. These sources are unmodified; if vendoring or modifying
them later, update both this statement and the actual source provision.

The eight other component records cover OtoLogic (including the processing
notice), Tabler, Tauri JavaScript API, the modified Tauri NSIS template, NSIS,
Modern UI, Vite's distributed modulepreload helper, and the Tauri NSIS utilities
plugin 0.5.3 (including its workspace plugin API). The plugin's upstream MIT
notice is retained; its semver and windows-sys dependency notices are also present
in the Rust section. The API and Vite versions
are checked against package-lock.json. Vite's own MIT grant is included because
the helper ships in the frontend; licenses of Vite's Node-only bundled build
tools are not copied. The Modern UI copyright symbol, previously decoded as a
replacement character, is restored. NSIS compression license material is retained
conservatively. The Microsoft WebView2 bootstrapper remains the original signed
Microsoft binary; it is packaged unchanged, not relabeled as open-source code.
Existing first-party image/icon files are not reassigned a third-party license.

## Review of the former 208 versus 180 entries

The 28 differences are host-side build dependencies, not runtime crates:

- HTML processing (16): bit-set, bit-vec, cssparser, dom_query, dtoa, dtoa-short,
  foldhash, html5ever, markup5ever, new_debug_unreachable, precomputed-hash,
  selectors, servo_arc, string_cache, tendril, web_atoms.
- Manifest/schema processing (8): camino, cargo_metadata, cargo-platform,
  dyn-clone, hashbrown 0.12.3, indexmap 1.9.3, rustc-hash, schemars 0.8.22.
- Build-side token/derive utilities (4): derive_more, proc-macro2, quote,
  unicode-ident.

Evidence inspected: tauri-codegen 2.7.3 src/context.rs (frontend embedding and
CSP mapping), src/embedded_assets.rs and src/image.rs (asset/image byte emission),
tauri-utils 2.10.2 src/html2.rs (parse_doc, serialize_doc and nonce insertion),
and their Cargo dependency/feature paths. HTML tools parse and serialize the
application's HTML; their parser, CSS selector implementations, entity lookup
tables and cache implementations are not copied into that HTML. Schema/manifest
tools produce metadata; token utilities produce Rust expressions and literals,
not copies of their own token-processing implementations. No include path that
embeds these 28 libraries' source or assets was found. They are excluded on that
basis, rather than solely because their dependency edge says build.

Tauri-generated glue and injected JavaScript come from Tauri components and
serialize-to-javascript already retained in the runtime list, plus the Tauri
installer template recorded separately. Their notices remain. Other build-only
macro crates are not linked runtime libraries. Revisit generated code/templates
and any build-time copied assets whenever the lockfile, features, packaging,
target, or generators change; compilation alone is not proof of exclusion.

## Deduplication and verification

Only whitespace is ignored when identifying identical texts. Copyright holders,
dates, additional terms and NOTICE contents remain part of identity. MIT headers
and the grant/disclaimer are stored as separate linked blocks so identical grants
can be shared while each copyright header remains connected to its component.
Every component links to all of its required blocks; different text is not merged
merely because it has the same SPDX name. Source text is HTML-escaped.

The migration compared 463 original text portions against their own component's
new references, with no missing portion among the 180 retained Rust components.
The original JSON was 2,142,824 bytes; the resulting standalone HTML is 458,667
bytes (about 78.6% smaller), including 180 Rust and 9 other component records and
138 unique linked text blocks. Tests cover distinct copyright preservation,
shared grants, escaping, reference integrity, no external resources, and MPL
source links. licenses:check recollects upstream files and compares the entire
output, detecting omitted or stale notices.

cargo-about was considered. Its target/build/dev filtering and templates are
useful, but do not replace the explicit source-copyright, missing-workspace-file,
NOTICE and non-Rust supplements needed here. A single Node generator using Cargo's
locked graph avoids another installed binary and keeps these policies in one
place. If adopting cargo-about later, retain these checks and supplements.
