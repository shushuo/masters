# Masters — web

Marketing landing page for [Masters](../README.md). Built with **Vite + React + TypeScript + Tailwind CSS**,
matching the stack chosen for the desktop app ([ADR-0002](../docs/adr/0002-desktop-shell.md)).

## Develop

```bash
pnpm install
pnpm dev        # start the dev server
pnpm build      # type-check + production build to dist/
pnpm preview    # preview the production build
```

The page content is driven by the data arrays at the top of `src/App.tsx` (features, comparison, steps);
edit those to update copy. The logo lives in `public/logo.svg` (copied from `../assets/logo.svg`).
