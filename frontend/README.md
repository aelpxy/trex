# Trex frontend

The Trex web interface uses React, React Router in SPA mode, TypeScript, Vite, and Tailwind CSS. It connects to the Rust API for accounts, workspaces, projects, persistent chats, streamed agent runs, attachments, library files, and credits.

Chat components display tool output, reasoning, plans, questions, network access requests, file changes, and live sandbox previews. The file panel supports browsing and editing sandbox files. The Scheduled page is currently a placeholder.

## Development

Start the backend using the [root setup instructions](../README.md), then run from this directory:

```sh
pnpm install
pnpm dev
```

Open the URL printed by Vite. The development server proxies `/api` to `http://127.0.0.1:8080` and removes the `/api` prefix. Set `TREX_URL` when starting Vite to use a different backend address.

```sh
pnpm typecheck
pnpm build
```

## Hosting

The build outputs static assets to `build/client`. Serve them with a fallback to `index.html` for client routes and proxy `/api` to the Rust backend, removing the prefix. `pnpm preview` previews the build locally; the development API proxy only applies to `pnpm dev`.

The API base defaults to `/api`; set `VITE_TREX_API` at build time to override it. Requests include the saved bearer token and selected workspace. Event streams reconnect using `Last-Event-ID`.

Sandbox previews are served by the backend's separate preview listener. Configure `TREX_PREVIEW_ADDR` and `TREX_PREVIEW_URL` on the backend for the hosting environment; each preview uses its own subdomain.

## Structure

- `app/routes`: authentication, home, chat, library, scheduled placeholder, and admin pages.
- `app/components/chat`: conversation UI and streamed event handling.
- `app/components/files`: sandbox file tree, editor, and live preview panel.
- `app/components/workspace`: account and workspace state.
- `app/components/appearance`: theme and appearance settings.
- `app/components/ui` and `app/app.css`: shared controls and styles.
- `app/lib/api.ts` and `app/lib/trex.ts`: API transport and typed operations.

The interface uses bundled Geist and Geist Mono fonts and supports light and dark themes. Tokens, workspace selection, and UI preferences are saved locally; conversations and library files are stored by the backend.
