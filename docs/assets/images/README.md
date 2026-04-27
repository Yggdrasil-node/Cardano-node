# Yggdrasil docs site image assets

This directory holds the image assets referenced by the docs site
(`docs/_config.yml`, `docs/_includes/header_custom.html`,
`docs/_includes/head_custom.html`).

| File | Purpose | Where it shows up |
| --- | --- | --- |
| `Yggrasil_banner.png` | Wide hero banner with the "YggdrasilNode — A Cardano Node Project Written In Rust" title | Top of [docs/index.md](../../index.md) and [README.md](../../../README.md) |
| `Yggrasil_logo.png` | Square tree-of-life mark, no text | just-the-docs sidebar `logo:`, used by `gh_edit_link` and `aux_links` icon |
| `favicon.png` | 32×32 PNG (or 64×64 retina) cropped from `Yggrasil_logo.png` | `<link rel="icon">` injected by `_includes/head_custom.html` |

## Dropping the files in

Save the assets the maintainer uploaded to:

```
docs/assets/images/Yggrasil_banner.png   # ~1428×590 wide hero, transparent or dark background
docs/assets/images/Yggrasil_logo.png     # ~512×512 square, transparent background
docs/assets/images/favicon.png            # 64×64 or 32×32 square, transparent background
```

The `gh-pages` workflow (`.github/workflows/pages.yml`) bundles
everything under `docs/` into the deployed site, so once the PNGs are
committed they'll appear on every page after the next deploy.

## Locally previewing

```bash
cd docs
bundle install
bundle exec jekyll serve
```

Then open <http://127.0.0.1:4000/Cardano-node/> — the banner appears
above the page heading and the logo appears in the sidebar.
