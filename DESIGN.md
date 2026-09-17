# Description card typography and layout

Applied the local `better-typography` and `better-layout` skills to the native Rust raster renderer. CSS-specific guidance is implemented through font metrics and pixel layout; production still requires no browser.

| Severity | Location | Before | After | Why |
| --- | --- | --- | --- | --- |
| Medium | `src/render.rs:136` | HTML hyperlinks displayed as bracketed Markdown references; long URL slugs broke across lines | Annotated inline text, blue underlined labels, compact canonical Chicago URLs; original destinations retained in alt text | Render links as readable content and preserve destinations |
| Medium | `src/render.rs:261` | Uniform font weight; title wrapping driven only by width | Real semibold face, 54px balanced heading at 1.1 line height, 32px body at 1.5 line height | Establish clear hierarchy and comfortable reading measure |
| Medium | `src/render.rs:307` | Four metadata lines competed with the description | Two-column metadata panel, paired labels and values, dynamic value wrapping | Group related content with alignment and space |
| Low | `src/render.rs:441` | Pagination could split short paragraphs arbitrarily | Keep fitting paragraphs together; split oversized paragraphs without dropping text or link destinations | Preserve reading flow and full content |

Verified: visually inspected the full ADU card at 1200px width; no clipping, footnote syntax, or broken long Chicago URL slugs. Automated regression checks cover HTML entities, link punctuation, original URLs in alt text, URL query/fragment preservation, long-token wrapping, and multipage text preservation. Existing post and persistence tests also pass (13 total), as does Clippy with warnings denied.

Not verified: the new image inside Bluesky's mobile viewer, RTL text shaping, and 200% browser zoom (this is a raster artifact, not a responsive webpage). The earlier live test post remains unchanged. Preview: `preview/polished/description-1.png`.

Approve for the inspected English-language image layout.
