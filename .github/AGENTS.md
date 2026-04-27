# Guidance for `.github/` workflows and infrastructure files.

## Scope
- `.github/workflows/*.yml` — CI and Pages deployment workflows.
- `.github/CLAUDE.md` — agent context loader.
- Other `.github/` infrastructure (issue templates, code owners, etc.).

## Rules *Non-Negotiable*
- Workflows MUST use pinned major-version action references (e.g.
  `actions/checkout@v4`, not `@main`).
- The Pages workflow MUST use the GitHub-managed Pages deployment
  flow (`actions/configure-pages` + `actions/upload-pages-artifact` +
  `actions/deploy-pages`) and MUST set `id: pages` on the
  `configure-pages` step so `${{ steps.pages.outputs.base_path }}`
  is populated for the Jekyll `--baseurl` flag.
- `permissions:` blocks MUST be present and minimal. Pages
  deployment requires `pages: write` and `id-token: write`; nothing
  more.
- The CI workflow (`ci.yml`) MUST keep running `cargo check-all`,
  `cargo test-all`, and `cargo lint` as the canonical gates.
- Always read the folder-specific `**/AGENTS.md` files. They MUST
  stay current and MUST remain operational rather than long-form
  documentation. If the folder context is outdated, missing, or
  incorrect, update the relevant `AGENTS.md` file.

## Pages workflow specifics
- Source files live under `docs/`; the Jekyll site uses
  `just-the-docs` via `remote_theme:`.
- The `_config.yml` `baseurl` defaults to `/Cardano-node` for local
  preview; the workflow overrides it via the `--baseurl` flag with
  the value computed by `actions/configure-pages`.
- `bundle-cache: true` is enabled for fast dependency resolution.
  The `Gemfile` MUST stay compatible with the runner's Ruby
  (currently 3.2).
- Plugins required for the build to succeed: `jekyll-remote-theme`,
  `jekyll-include-cache`, `jekyll-seo-tag`. All three MUST appear in
  `docs/_config.yml` `plugins:` and in `docs/Gemfile`.
- When the repository is forked or the project name changes, update
  `docs/_config.yml` `baseurl`, `gh_edit_repository`, and
  `aux_links` accordingly.

## Verification
- Pages workflow build succeeds end-to-end on push to `main` when
  `docs/**` changes or when manually dispatched.
- Deployed site renders the just-the-docs theme (sidebar nav,
  search, syntax-highlighted code, copy-button).
- `cargo lint` clean (Rust workspace gates unaffected by docs
  changes).
