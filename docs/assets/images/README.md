# Yggdrasil docs site image assets

This directory holds the image assets referenced by the docs site
(`docs/_config.yml`, `docs/_includes/header_custom.html`,
`docs/_includes/head_custom.html`).

| File | Purpose | Where it shows up |
| --- | --- | --- |
| `Yggrasil_banner.png` | Wide hero banner with the "YggdrasilNode — A Cardano Node Project Written In Rust" title | Top of [docs/index.md](../../index.md) (gated by `hero: true` front-matter) and [README.md](../../../README.md). Also the Open Graph / Twitter Card image set in [`_includes/head_custom.html`](../../_includes/head_custom.html). |
| `Yggrasil_logo.png` | Square tree-of-life mark, no text | just-the-docs sidebar `logo:` in [`_config.yml`](../../_config.yml) AND the `<link rel="icon">` favicon (set in [`_includes/head_custom.html`](../../_includes/head_custom.html)). One asset, two roles. |

## Dropping the files in

Both files are committed to the repo:

```
docs/assets/images/Yggrasil_banner.png   # 1428×590 wide hero, dark background
docs/assets/images/Yggrasil_logo.png     # 512×512 square, transparent background
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
